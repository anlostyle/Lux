//! Version priority settings: library rules (administrators), the per-user permission
//! (administrators) and the user's own rules.

use super::*;

use crate::application::version_priority::{
    ALL_LIBRARIES_SCOPE, VersionPriorityMode, VersionPriorityRule,
};

pub(super) fn api_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/admin/libraries/{library_id}/version-priority",
            get(admin_library_version_priority).put(admin_update_library_version_priority),
        )
        .route(
            "/api/v1/admin/libraries/{library_id}/version-priority/preview",
            post(admin_preview_library_version_priority),
        )
        .route(
            "/api/v1/admin/users/{user_id}/version-priority",
            get(admin_user_version_priority).put(admin_update_user_version_priority),
        )
        .route(
            "/api/v1/auth/version-priority",
            get(user_version_priority).put(user_update_version_priority),
        )
        .route(
            "/api/v1/auth/version-priority/preview",
            post(user_preview_version_priority),
        )
}

fn unavailable(headers: &HeaderMap) -> Response {
    api_error(
        headers,
        StatusCode::SERVICE_UNAVAILABLE,
        lux::ApiErrorCode::DatabaseUnavailable,
        "服务尚未就绪",
    )
    .into_response()
}

fn invalid(headers: &HeaderMap, message: &str) -> Response {
    api_error(
        headers,
        StatusCode::BAD_REQUEST,
        lux::ApiErrorCode::InvalidRequest,
        message,
    )
    .into_response()
}

fn not_found(headers: &HeaderMap, message: &str) -> Response {
    api_error(
        headers,
        StatusCode::NOT_FOUND,
        lux::ApiErrorCode::NotFound,
        message,
    )
    .into_response()
}

fn rule_json(rule: Option<&VersionPriorityRule>) -> Value {
    serde_json::to_value(rule.cloned().unwrap_or_default()).unwrap_or(Value::Null)
}

fn preview_json(item: &crate::application::catalog::CatalogItem) -> Value {
    json!({
        "itemId": item.id,
        "title": item.title,
        "sources": item.media_sources.iter().map(|source| json!({
            "id": source.id,
            "fileName": source.file_name,
            "editionName": source.edition_name,
            "qualityLabel": source.quality_label,
            "versionKey": source.version_key(),
            "partIndex": source.part_index(),
            "isDefault": source.is_default,
            "size": source.size,
            "bitrate": source.bitrate,
        })).collect::<Vec<_>>(),
    })
}

async fn library_exists(state: &AppState, library_id: &str) -> Result<bool, ()> {
    let database = state.database.as_ref().ok_or(())?;
    database
        .find_library(library_id)
        .await
        .map(|library| library.is_some())
        .map_err(|_| ())
}

async fn admin_library_version_priority(
    headers: HeaderMap,
    Path(library_id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    if let Err(response) = require_admin(&headers, &state, false).await {
        return response;
    }
    let Some(database) = state.database.as_ref() else {
        return unavailable(&headers);
    };
    match library_exists(&state, &library_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(&headers, "媒体库不存在"),
        Err(()) => return unavailable(&headers),
    }
    let snapshot = database.version_priority_snapshot();
    Json(json!({
        "libraryId": library_id,
        "rule": rule_json(snapshot.library_rules.get(&library_id)),
    }))
    .into_response()
}

async fn admin_update_library_version_priority(
    headers: HeaderMap,
    Path(library_id): Path<String>,
    State(state): State<AppState>,
    Json(rule): Json<VersionPriorityRule>,
) -> Response {
    if let Err(response) = require_admin(&headers, &state, true).await {
        return response;
    }
    let (Some(database), Some(catalog)) = (state.database.as_ref(), state.catalog.as_ref()) else {
        return unavailable(&headers);
    };
    match library_exists(&state, &library_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(&headers, "媒体库不存在"),
        Err(()) => return unavailable(&headers),
    }
    let rule = match rule.validated(false) {
        Ok(rule) => rule,
        Err(error) => return invalid(&headers, &error.to_string()),
    };
    // "Keep default" removes the rule: the stored order applies again.
    let stored = (rule.mode != VersionPriorityMode::Default).then_some(&rule);
    if database
        .set_library_version_priority(&library_id, stored)
        .await
        .is_err()
    {
        return unavailable(&headers);
    }
    let catalog = catalog.clone();
    let recompute_library_id = library_id.clone();
    tokio::spawn(async move {
        match catalog
            .recompute_library_default_sources(&recompute_library_id)
            .await
        {
            Ok(changed) => tracing::info!(
                library_id = %recompute_library_id,
                changed,
                "default sources recomputed for the library version priority"
            ),
            Err(error) => tracing::warn!(
                library_id = %recompute_library_id,
                %error,
                "default source recompute failed"
            ),
        }
    });
    Json(json!({
        "libraryId": library_id,
        "rule": rule_json(stored),
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRequest {
    item_id: String,
    #[serde(default)]
    rule: Option<VersionPriorityRule>,
}

async fn admin_preview_library_version_priority(
    headers: HeaderMap,
    Path(library_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<PreviewRequest>,
) -> Response {
    if let Err(response) = require_admin(&headers, &state, true).await {
        return response;
    }
    let Some(catalog) = state.catalog.as_ref() else {
        return unavailable(&headers);
    };
    let rule = match request.rule.map(|rule| rule.validated(false)).transpose() {
        Ok(rule) => rule,
        Err(error) => return invalid(&headers, &error.to_string()),
    };
    match catalog
        .preview_version_order(
            AccessPrincipal::server_admin(None),
            &request.item_id,
            rule.as_ref(),
            true,
        )
        .await
    {
        Ok(Some(item)) if item.library_id == library_id => {
            Json(preview_json(&item)).into_response()
        }
        Ok(_) => not_found(&headers, "条目不存在"),
        Err(_) => unavailable(&headers),
    }
}

fn user_rules_json(
    snapshot: &crate::application::version_priority::VersionPrioritySnapshot,
    user_id: &str,
    is_admin: bool,
) -> Value {
    let rules = snapshot
        .users
        .get(user_id)
        .map(|user| {
            user.rules
                .iter()
                .map(|(scope, rule)| (scope.clone(), rule_json(Some(rule))))
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    json!({
        "canCustomize": snapshot.user_can_customize(user_id, is_admin),
        "rules": rules,
    })
}

async fn admin_user_version_priority(
    headers: HeaderMap,
    Path(user_id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    if let Err(response) = require_admin(&headers, &state, false).await {
        return response;
    }
    let Some(database) = state.database.as_ref() else {
        return unavailable(&headers);
    };
    let user = match database.find_user_by_id(&user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return not_found(&headers, "用户不存在"),
        Err(_) => return unavailable(&headers),
    };
    let snapshot = database.version_priority_snapshot();
    let mut body = user_rules_json(&snapshot, &user_id, user.is_admin);
    // Report the stored permission so administrators can see what non-admin rights apply.
    body["canCustomize"] = json!(
        snapshot
            .users
            .get(&user_id)
            .is_none_or(|settings| settings.can_customize)
    );
    Json(body).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserPermissionRequest {
    can_customize: bool,
}

async fn admin_update_user_version_priority(
    headers: HeaderMap,
    Path(user_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<UserPermissionRequest>,
) -> Response {
    if let Err(response) = require_admin(&headers, &state, true).await {
        return response;
    }
    let Some(database) = state.database.as_ref() else {
        return unavailable(&headers);
    };
    match database.find_user_by_id(&user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(&headers, "用户不存在"),
        Err(_) => return unavailable(&headers),
    }
    if database
        .set_user_version_priority_permission(&user_id, request.can_customize)
        .await
        .is_err()
    {
        return unavailable(&headers);
    }
    Json(json!({ "canCustomize": request.can_customize })).into_response()
}

async fn user_version_priority(headers: HeaderMap, State(state): State<AppState>) -> Response {
    let user = match users::require_web_user(&headers, &state).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Some(database) = state.database.as_ref() else {
        return unavailable(&headers);
    };
    let snapshot = database.version_priority_snapshot();
    Json(user_rules_json(
        &snapshot,
        &user.id.to_string(),
        user.is_admin,
    ))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserRuleRequest {
    /// A library id, or `*` for every library.
    scope: String,
    rule: VersionPriorityRule,
}

async fn user_scope_allowed(
    state: &AppState,
    principal: AccessPrincipal,
    scope: &str,
) -> Result<bool, ()> {
    if scope == ALL_LIBRARIES_SCOPE {
        return Ok(true);
    }
    let access = state.access.as_ref().ok_or(())?;
    access
        .accessible_library_ids(principal)
        .await
        .map(|ids| ids.iter().any(|id| id == scope))
        .map_err(|_| ())
}

async fn user_update_version_priority(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<UserRuleRequest>,
) -> Response {
    let user = match users::require_web_user(&headers, &state).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if let Err(response) = users::require_web_csrf(&headers, &state).await {
        return response;
    }
    let Some(database) = state.database.as_ref() else {
        return unavailable(&headers);
    };
    let user_id = user.id.to_string();
    if !database
        .version_priority_snapshot()
        .user_can_customize(&user_id, user.is_admin)
    {
        return api_error(
            &headers,
            StatusCode::FORBIDDEN,
            lux::ApiErrorCode::PermissionDenied,
            "没有自定义版本优先的权限",
        )
        .into_response();
    }
    let principal = AccessPrincipal::new(user.id, user.is_admin);
    match user_scope_allowed(&state, principal, &request.scope).await {
        Ok(true) => {}
        Ok(false) => return not_found(&headers, "媒体库不存在"),
        Err(()) => return unavailable(&headers),
    }
    let rule = match request.rule.validated(true) {
        Ok(rule) => rule,
        Err(error) => return invalid(&headers, &error.to_string()),
    };
    // "Follow the library" removes the user's rule for that scope.
    let stored = (rule.mode != VersionPriorityMode::Inherit).then_some(&rule);
    if database
        .set_user_version_priority(&user_id, &request.scope, stored)
        .await
        .is_err()
    {
        return unavailable(&headers);
    }
    let snapshot = database.version_priority_snapshot();
    Json(user_rules_json(&snapshot, &user_id, user.is_admin)).into_response()
}

async fn user_preview_version_priority(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<PreviewRequest>,
) -> Response {
    let user = match users::require_web_user(&headers, &state).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if let Err(response) = users::require_web_csrf(&headers, &state).await {
        return response;
    }
    let Some(catalog) = state.catalog.as_ref() else {
        return unavailable(&headers);
    };
    let rule = match request.rule.map(|rule| rule.validated(true)).transpose() {
        Ok(rule) => rule.filter(|rule| rule.mode != VersionPriorityMode::Inherit),
        Err(error) => return invalid(&headers, &error.to_string()),
    };
    match catalog
        .preview_version_order(
            AccessPrincipal::new(user.id, user.is_admin),
            &request.item_id,
            rule.as_ref(),
            false,
        )
        .await
    {
        Ok(Some(item)) => Json(preview_json(&item)).into_response(),
        Ok(None) => not_found(&headers, "条目不存在"),
        Err(_) => unavailable(&headers),
    }
}
