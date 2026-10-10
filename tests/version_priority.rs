use std::time::Duration;

use luxd::{
    api::{AppState, app_with_state},
    application::{libraries::LibraryService, setup::SetupService},
    auth::{emby::EmbyAuthService, sessions::WebAuthService, users::UserStore},
    config::Config,
    library::LibraryKind,
    storage::Database,
};
use reqwest::header::{AUTHORIZATION, COOKIE, SET_COOKIE};
use serde_json::{Value, json};
use tokio::net::TcpListener;

fn cookie_value(headers: &reqwest::header::HeaderMap, name: &str) -> String {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| {
            let (pair, _) = value.split_once(';')?;
            let (cookie_name, cookie_value) = pair.split_once('=')?;
            (cookie_name == name).then(|| cookie_value.to_owned())
        })
        .expect("expected cookie")
}

struct WebSession {
    cookie: String,
    csrf: String,
}

async fn web_login(
    client: &reqwest::Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<WebSession, Box<dyn std::error::Error>> {
    let login = client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await?;
    assert_eq!(login.status(), reqwest::StatusCode::OK);
    let session = cookie_value(login.headers(), "lux_session");
    let csrf = cookie_value(login.headers(), "lux_csrf");
    Ok(WebSession {
        cookie: format!("lux_session={session}; lux_csrf={csrf}"),
        csrf,
    })
}

async fn emby_login(
    client: &reqwest::Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = client
        .post(format!("{base_url}/emby/Users/AuthenticateByName"))
        .header(
            AUTHORIZATION,
            format!(
                r#"Emby Client="VersionTest", Device="Mac", DeviceId="{username}-device", Version="1""#
            ),
        )
        .json(&json!({ "Username": username, "Pw": password }))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    Ok(response.json::<Value>().await?["AccessToken"]
        .as_str()
        .ok_or("missing access token")?
        .to_owned())
}

async fn lux_editions(
    client: &reqwest::Client,
    base_url: &str,
    session: &WebSession,
    item_id: &str,
) -> Result<Vec<(String, bool)>, Box<dyn std::error::Error>> {
    let body: Value = client
        .get(format!("{base_url}/api/v1/items/{item_id}"))
        .header(COOKIE, &session.cookie)
        .send()
        .await?
        .json()
        .await?;
    let sources = body["mediaSources"]
        .as_array()
        .or_else(|| body["item"]["mediaSources"].as_array())
        .ok_or("missing media sources")?;
    Ok(sources
        .iter()
        .map(|source| {
            (
                source["editionName"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                source["isDefault"].as_bool().unwrap_or(false),
            )
        })
        .collect())
}

async fn emby_editions(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    user_id: &str,
    item_id: &str,
) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let item: Value = client
        .get(format!("{base_url}/emby/Users/{user_id}/Items/{item_id}"))
        .header("X-Emby-Token", token)
        .send()
        .await?
        .json()
        .await?;
    let playback: Value = client
        .post(format!(
            "{base_url}/emby/Items/{item_id}/PlaybackInfo?UserId={user_id}"
        ))
        .header("X-Emby-Token", token)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let editions = |value: &Value| {
        value["MediaSources"]
            .as_array()
            .map(|sources| {
                sources
                    .iter()
                    .map(|source| source["Edition"].as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    Ok((editions(&item), editions(&playback)))
}

#[tokio::test]
async fn version_priority_follows_user_then_library_rules_across_lux_and_emby()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let config = Config {
        http_addr: "127.0.0.1:8097".parse()?,
        config_dir: temp_dir.path().join("config"),
    };
    let root_path = temp_dir.path().join("Movies");
    tokio::fs::create_dir_all(&root_path).await?;

    let database = Database::connect(&config).await?;
    let setup = SetupService::new(database.clone())?;
    let _admin = setup.complete("Admin", "Admin", "correct password").await?;
    let viewer = UserStore::new(database.clone())?
        .create_user("viewer", "Viewer", "viewer password", false)
        .await?;
    let libraries = LibraryService::new(database.clone());
    let library = libraries
        .create_library("Movies", LibraryKind::Movie, false)
        .await?;
    let root = libraries
        .add_root(library.id, root_path.to_str().ok_or("non-utf8 root")?)
        .await?
        .root;
    let library_id = library.id.to_string();
    sqlx::query("INSERT INTO user_library_access (user_id, library_id, can_view) VALUES (?, ?, 1)")
        .bind(viewer.id.to_string())
        .bind(&library_id)
        .execute(database.pool())
        .await?;
    let item_id = "film".to_owned();
    sqlx::query(
        "INSERT INTO media_items (
             id, library_id, item_type, title, sort_title, identification_status,
             has_available_source
         ) VALUES ('film', ?, 'MOVIE', 'Film', 'film', 'LOCAL_CONFIRMED', 1)",
    )
    .bind(&library_id)
    .execute(database.pool())
    .await?;
    // Inserted oldest first: Theatrical is the stored default.
    for (index, edition) in ["Theatrical", "Extended", "Directors Cut"]
        .iter()
        .enumerate()
    {
        sqlx::query(
            "INSERT INTO filesystem_entries (
                 id, library_root_id, relative_path, entry_kind, size, modified_at,
                 last_seen_generation
             ) VALUES (?, ?, ?, 'FILE', 10, 1, 'generation')",
        )
        .bind(format!("entry-{index}"))
        .bind(root.id.to_string())
        .bind(format!("Film/Film-{edition}.mkv"))
        .execute(database.pool())
        .await?;
        sqlx::query(
            "INSERT INTO media_sources (
                 id, item_id, source_kind, filesystem_entry_id, edition_name, container,
                 size, is_default, probe_status
             ) VALUES (?, 'film', 'LOCAL_FILE', ?, ?, 'mkv', 10, ?, 'READY')",
        )
        .bind(format!("source-{index}"))
        .bind(format!("entry-{index}"))
        .bind(*edition)
        .bind(i64::from(index == 0))
        .execute(database.pool())
        .await?;
    }
    let viewer_id = viewer.id.to_string();

    let web_auth = WebAuthService::new(database.clone())?;
    let emby_auth = EmbyAuthService::new(database.clone())?;
    let app = app_with_state(AppState::ready(
        config,
        database.clone(),
        setup,
        web_auth,
        emby_auth,
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let base_url = format!("http://{address}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let admin = web_login(&client, &base_url, "admin", "correct password").await?;
    let viewer_web = web_login(&client, &base_url, "viewer", "viewer password").await?;
    let viewer_token = emby_login(&client, &base_url, "viewer", "viewer password").await?;

    // Without rules the oldest version is first and default.
    let editions = lux_editions(&client, &base_url, &viewer_web, &item_id).await?;
    assert_eq!(editions[0], ("Theatrical".to_owned(), true));
    assert_eq!(editions.iter().filter(|(_, default)| *default).count(), 1);

    // Library rule: Extended first, then Directors Cut.
    let library_rule = json!({
        "mode": "custom",
        "custom": {
            "keywordGroups": [["Extended"], ["Directors Cut"]],
            "subtitle": "ignore",
            "subtitleKeywords": [],
            "tieBreakers": ["resolution", "size"]
        }
    });
    let stored = client
        .put(format!(
            "{base_url}/api/v1/admin/libraries/{library_id}/version-priority"
        ))
        .header(COOKIE, &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .json(&library_rule)
        .send()
        .await?;
    assert_eq!(stored.status(), reqwest::StatusCode::OK);
    let stored: Value = stored.json().await?;
    assert_eq!(stored["rule"], library_rule);
    let read_back: Value = client
        .get(format!(
            "{base_url}/api/v1/admin/libraries/{library_id}/version-priority"
        ))
        .header(COOKIE, &admin.cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(read_back["rule"], library_rule);

    let editions = lux_editions(&client, &base_url, &viewer_web, &item_id).await?;
    assert_eq!(
        editions,
        [
            ("Extended".to_owned(), true),
            ("Directors Cut".to_owned(), false),
            ("Theatrical".to_owned(), false)
        ]
    );
    let (emby_item, emby_playback) =
        emby_editions(&client, &base_url, &viewer_token, &viewer_id, &item_id).await?;
    assert_eq!(emby_item, ["Extended", "Directors Cut", "Theatrical"]);
    assert_eq!(emby_playback, ["Extended", "Directors Cut", "Theatrical"]);

    // The library rule is also stored as the default source, in the background.
    let mut stored_default = String::new();
    for _ in 0..100 {
        stored_default = sqlx::query_scalar(
            "SELECT edition_name FROM media_sources WHERE item_id = ? AND is_default = 1",
        )
        .bind(&item_id)
        .fetch_one(database.pool())
        .await?;
        if stored_default == "Extended" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(stored_default, "Extended");

    // The viewer's own rule wins for the viewer only.
    let user_rule = client
        .put(format!("{base_url}/api/v1/auth/version-priority"))
        .header(COOKIE, &viewer_web.cookie)
        .header("x-csrf-token", &viewer_web.csrf)
        .json(&json!({
            "scope": "*",
            "rule": { "mode": "custom", "custom": { "keywordGroups": [["Theatrical"]] } }
        }))
        .send()
        .await?;
    assert_eq!(user_rule.status(), reqwest::StatusCode::OK);
    let user_rule: Value = user_rule.json().await?;
    assert_eq!(user_rule["canCustomize"], true);
    assert_eq!(user_rule["rules"]["*"]["mode"], "custom");
    let editions = lux_editions(&client, &base_url, &viewer_web, &item_id).await?;
    assert_eq!(editions[0], ("Theatrical".to_owned(), true));
    let (emby_item, emby_playback) =
        emby_editions(&client, &base_url, &viewer_token, &viewer_id, &item_id).await?;
    assert_eq!(emby_item[0], "Theatrical");
    assert_eq!(emby_playback[0], "Theatrical");
    let editions = lux_editions(&client, &base_url, &admin, &item_id).await?;
    assert_eq!(editions[0], ("Extended".to_owned(), true));

    // A library-scoped "follow the library" rule falls back to the user's all-library rule;
    // the preview shows what the viewer gets.
    let preview: Value = client
        .post(format!("{base_url}/api/v1/auth/version-priority/preview"))
        .header(COOKIE, &viewer_web.cookie)
        .header("x-csrf-token", &viewer_web.csrf)
        .json(&json!({ "itemId": item_id }))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(preview["sources"][0]["editionName"], "Theatrical");
    let admin_preview: Value = client
        .post(format!(
            "{base_url}/api/v1/admin/libraries/{library_id}/version-priority/preview"
        ))
        .header(COOKIE, &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .json(&json!({
            "itemId": item_id,
            "rule": { "mode": "custom", "custom": { "keywordGroups": [["Directors Cut"]] } }
        }))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(admin_preview["sources"][0]["editionName"], "Directors Cut");

    // Without permission the viewer's rule is ignored and cannot be changed.
    let permission = client
        .put(format!(
            "{base_url}/api/v1/admin/users/{viewer_id}/version-priority"
        ))
        .header(COOKIE, &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .json(&json!({ "canCustomize": false }))
        .send()
        .await?;
    assert_eq!(permission.status(), reqwest::StatusCode::OK);
    let editions = lux_editions(&client, &base_url, &viewer_web, &item_id).await?;
    assert_eq!(editions[0], ("Extended".to_owned(), true));
    let denied = client
        .put(format!("{base_url}/api/v1/auth/version-priority"))
        .header(COOKIE, &viewer_web.cookie)
        .header("x-csrf-token", &viewer_web.csrf)
        .json(&json!({ "scope": "*", "rule": { "mode": "quality" } }))
        .send()
        .await?;
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);

    // Clearing the library rule restores the stored order and default.
    let cleared = client
        .put(format!(
            "{base_url}/api/v1/admin/libraries/{library_id}/version-priority"
        ))
        .header(COOKIE, &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .json(&json!({ "mode": "default" }))
        .send()
        .await?;
    assert_eq!(cleared.status(), reqwest::StatusCode::OK);
    assert_eq!(cleared.json::<Value>().await?["rule"]["mode"], "default");
    for _ in 0..100 {
        stored_default = sqlx::query_scalar(
            "SELECT edition_name FROM media_sources WHERE item_id = ? AND is_default = 1",
        )
        .bind(&item_id)
        .fetch_one(database.pool())
        .await?;
        if stored_default == "Theatrical" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(stored_default, "Theatrical");
    let editions = lux_editions(&client, &base_url, &viewer_web, &item_id).await?;
    assert_eq!(editions[0], ("Theatrical".to_owned(), true));

    // Library rules reject the user-only "inherit" mode and unknown fields.
    for invalid in [
        json!({ "mode": "inherit" }),
        json!({ "mode": "custom", "extra": 1 }),
    ] {
        let response = client
            .put(format!(
                "{base_url}/api/v1/admin/libraries/{library_id}/version-priority"
            ))
            .header(COOKIE, &admin.cookie)
            .header("x-csrf-token", &admin.csrf)
            .json(&invalid)
            .send()
            .await?;
        assert!(response.status().is_client_error());
    }

    server.abort();
    Ok(())
}
