use super::*;

use crate::application::version_priority::{
    UserVersionPriority, VersionPriorityRule, VersionPrioritySnapshot,
};

impl Database {
    /// The rules currently in effect. Empty until [`Self::reload_version_priority`] ran.
    pub fn version_priority_snapshot(&self) -> Arc<VersionPrioritySnapshot> {
        self.version_priority
            .read()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    /// Reads every stored rule into memory. Called at startup and after each write.
    pub async fn reload_version_priority(&self) -> Result<(), StorageError> {
        let mut snapshot = VersionPrioritySnapshot::default();
        let library_rows = self
            .query_as::<(String, String)>(
                "SELECT library_id, rule_json FROM library_version_priority",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|source| StorageError::Sqlx {
                path: self.path.clone(),
                source,
            })?;
        for (library_id, rule_json) in library_rows {
            if let Some(rule) = parse_stored_rule(&rule_json) {
                snapshot.library_rules.insert(library_id, rule);
            }
        }
        let settings = self
            .query_as::<(String, i64)>(
                "SELECT user_id, can_customize FROM user_version_priority_settings",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|source| StorageError::Sqlx {
                path: self.path.clone(),
                source,
            })?;
        for (user_id, can_customize) in settings {
            snapshot
                .users
                .entry(user_id)
                .or_insert_with(|| UserVersionPriority {
                    can_customize: true,
                    ..UserVersionPriority::default()
                })
                .can_customize = can_customize != 0;
        }
        let user_rows = self
            .query_as::<(String, String, String)>(
                "SELECT user_id, scope_id, rule_json FROM user_version_priority_rules",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|source| StorageError::Sqlx {
                path: self.path.clone(),
                source,
            })?;
        for (user_id, scope_id, rule_json) in user_rows {
            if let Some(rule) = parse_stored_rule(&rule_json) {
                snapshot
                    .users
                    .entry(user_id)
                    .or_insert_with(|| UserVersionPriority {
                        can_customize: true,
                        ..UserVersionPriority::default()
                    })
                    .rules
                    .insert(scope_id, rule);
            }
        }
        if let Ok(mut current) = self.version_priority.write() {
            *current = Arc::new(snapshot);
        }
        Ok(())
    }

    /// Stores (or with `None` removes) the rule of a library.
    pub async fn set_library_version_priority(
        &self,
        library_id: &str,
        rule: Option<&VersionPriorityRule>,
    ) -> Result<(), StorageError> {
        match rule {
            Some(rule) => {
                let rule_json = serde_json::to_string(rule)
                    .map_err(|error| StorageError::Serialization(error.to_string()))?;
                self.query(
                    "INSERT INTO library_version_priority (library_id, rule_json)
                     VALUES (?, ?)
                     ON CONFLICT(library_id) DO UPDATE SET
                         rule_json = excluded.rule_json, updated_at = unixepoch()",
                )
                .bind(library_id)
                .bind(rule_json)
                .execute(&self.pool)
                .await
            }
            None => {
                self.query("DELETE FROM library_version_priority WHERE library_id = ?")
                    .bind(library_id)
                    .execute(&self.pool)
                    .await
            }
        }
        .map_err(|source| StorageError::Sqlx {
            path: self.path.clone(),
            source,
        })?;
        self.reload_version_priority().await
    }

    /// Stores (or with `None` removes) a user's rule for a library or `*`.
    pub async fn set_user_version_priority(
        &self,
        user_id: &str,
        scope_id: &str,
        rule: Option<&VersionPriorityRule>,
    ) -> Result<(), StorageError> {
        match rule {
            Some(rule) => {
                let rule_json = serde_json::to_string(rule)
                    .map_err(|error| StorageError::Serialization(error.to_string()))?;
                self.query(
                    "INSERT INTO user_version_priority_rules (user_id, scope_id, rule_json)
                     VALUES (?, ?, ?)
                     ON CONFLICT(user_id, scope_id) DO UPDATE SET
                         rule_json = excluded.rule_json, updated_at = unixepoch()",
                )
                .bind(user_id)
                .bind(scope_id)
                .bind(rule_json)
                .execute(&self.pool)
                .await
            }
            None => {
                self.query(
                    "DELETE FROM user_version_priority_rules WHERE user_id = ? AND scope_id = ?",
                )
                .bind(user_id)
                .bind(scope_id)
                .execute(&self.pool)
                .await
            }
        }
        .map_err(|source| StorageError::Sqlx {
            path: self.path.clone(),
            source,
        })?;
        self.reload_version_priority().await
    }

    pub async fn set_user_version_priority_permission(
        &self,
        user_id: &str,
        can_customize: bool,
    ) -> Result<(), StorageError> {
        self.query(
            "INSERT INTO user_version_priority_settings (user_id, can_customize)
             VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET
                 can_customize = excluded.can_customize, updated_at = unixepoch()",
        )
        .bind(user_id)
        .bind(database_flag(can_customize))
        .execute(&self.pool)
        .await
        .map_err(|source| StorageError::Sqlx {
            path: self.path.clone(),
            source,
        })?;
        self.reload_version_priority().await
    }

    /// Items of a library with more than one source, in id order after `after_id`.
    pub(crate) async fn list_multi_source_item_ids(
        &self,
        library_id: &str,
        after_id: &str,
        limit: i64,
    ) -> Result<Vec<String>, StorageError> {
        self.query_scalar::<String>(
            "SELECT mi.id FROM media_items mi
             WHERE mi.library_id = ? AND mi.id > ? AND mi.removed_at IS NULL
               AND (SELECT COUNT(*) FROM media_sources ms WHERE ms.item_id = mi.id) > 1
             ORDER BY mi.id
             LIMIT ?",
        )
        .bind(library_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|source| StorageError::Sqlx {
            path: self.path.clone(),
            source,
        })
    }

    /// Makes `source_id` the only default source of `item_id`.
    pub(crate) async fn set_default_media_source(
        &self,
        item_id: &str,
        source_id: &str,
    ) -> Result<bool, StorageError> {
        self.query(
            "UPDATE media_sources
             SET is_default = CASE WHEN id = ? THEN 1 ELSE 0 END, updated_at = unixepoch()
             WHERE item_id = ?
               AND is_default <> CASE WHEN id = ? THEN 1 ELSE 0 END",
        )
        .bind(source_id)
        .bind(item_id)
        .bind(source_id)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|source| StorageError::Sqlx {
            path: self.path.clone(),
            source,
        })
    }
}

fn parse_stored_rule(rule_json: &str) -> Option<VersionPriorityRule> {
    match serde_json::from_str::<VersionPriorityRule>(rule_json) {
        Ok(rule) => Some(rule),
        Err(error) => {
            tracing::warn!(%error, "ignoring an unreadable version priority rule");
            None
        }
    }
}
