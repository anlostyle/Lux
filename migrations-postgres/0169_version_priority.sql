-- Version priority rules decide which version (media source) of an item plays by default
-- and the order versions are listed in. Library rules are set by administrators; users
-- may override them for themselves unless an administrator turned that off.
CREATE TABLE library_version_priority (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    rule_json TEXT NOT NULL,
    updated_at BIGINT NOT NULL DEFAULT (unixepoch())
);

-- A missing row means the user may customize version priority.
CREATE TABLE user_version_priority_settings (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    can_customize BIGINT NOT NULL DEFAULT 1 CHECK (can_customize IN (0, 1)),
    updated_at BIGINT NOT NULL DEFAULT (unixepoch())
);

-- scope_id is a library id or '*' for every library.
CREATE TABLE user_version_priority_rules (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_id TEXT NOT NULL CHECK (length(scope_id) BETWEEN 1 AND 64),
    rule_json TEXT NOT NULL,
    updated_at BIGINT NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, scope_id)
);
