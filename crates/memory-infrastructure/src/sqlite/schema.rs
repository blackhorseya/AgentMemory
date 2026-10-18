//! DDL. Hot query fields are real columns; metadata entities additionally
//! keep their full serialized form in `doc` so the mapping stays in serde.

use rusqlite::Connection;

const INSTANCE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    team_id TEXT NOT NULL, agent_id TEXT NOT NULL, user_id TEXT NOT NULL, task_id TEXT,
    session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
    timestamp_ms INTEGER NOT NULL, recorded_at_ms INTEGER NOT NULL, version INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_conv_scope ON conversations(team_id, agent_id, user_id, recorded_at_ms);
CREATE INDEX IF NOT EXISTS ix_conv_session ON conversations(session_id, seq);
CREATE VIRTUAL TABLE IF NOT EXISTS conversations_fts USING fts5(tokens, id UNINDEXED);

CREATE TABLE IF NOT EXISTS atoms (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    team_id TEXT NOT NULL, agent_id TEXT NOT NULL, user_id TEXT NOT NULL, task_id TEXT,
    session_id TEXT, atom_type TEXT NOT NULL, content TEXT NOT NULL, background TEXT,
    priority INTEGER NOT NULL, source_ids TEXT NOT NULL, version INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, embedding BLOB
);
CREATE INDEX IF NOT EXISTS ix_atoms_scope ON atoms(team_id, agent_id, user_id, updated_at_ms);
CREATE INDEX IF NOT EXISTS ix_atoms_profile ON atoms(team_id, agent_id, seq);
CREATE VIRTUAL TABLE IF NOT EXISTS atoms_fts USING fts5(tokens, id UNINDEXED);
-- Bookkeeping for derived indexes (e.g. the sqlite-vec atom index's dims).
CREATE TABLE IF NOT EXISTS index_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS profile_files (
    team_id TEXT NOT NULL, agent_id TEXT NOT NULL, path TEXT NOT NULL, version INTEGER NOT NULL,
    content TEXT NOT NULL, is_head INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (team_id, agent_id, path, version)
);
CREATE INDEX IF NOT EXISTS ix_profile_head ON profile_files(team_id, agent_id, is_head, path);

CREATE TABLE IF NOT EXISTS memory_prompts (
    id TEXT PRIMARY KEY, layer TEXT NOT NULL, status TEXT NOT NULL, created_at_ms INTEGER NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS prompt_settings (
    target_type TEXT NOT NULL, team_id TEXT NOT NULL, agent_id TEXT NOT NULL, layer TEXT NOT NULL,
    memory_prompt_id TEXT NOT NULL, updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (target_type, team_id, agent_id, layer)
);
CREATE TABLE IF NOT EXISTS prompt_logs (
    id TEXT PRIMARY KEY, memory_prompt_id TEXT, layer TEXT NOT NULL, target_type TEXT NOT NULL,
    team_id TEXT, agent_id TEXT, action TEXT NOT NULL, created_at_ms INTEGER NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS generation_logs (
    log_id TEXT PRIMARY KEY, layer TEXT NOT NULL, status TEXT NOT NULL, started_at_ms INTEGER NOT NULL, doc TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_genlog_time ON generation_logs(started_at_ms DESC, log_id DESC);
CREATE TABLE IF NOT EXISTS generation_refs (
    layer TEXT NOT NULL, record_id TEXT NOT NULL, log_id TEXT NOT NULL, started_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_genref ON generation_refs(layer, record_id, started_at_ms DESC);

CREATE TABLE IF NOT EXISTS skills (
    skill_id TEXT NOT NULL, version INTEGER NOT NULL, is_head INTEGER NOT NULL, name TEXT NOT NULL,
    status TEXT NOT NULL, team_id TEXT NOT NULL, owner_agent_id TEXT NOT NULL, owner_user_id TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL, created_at_ms INTEGER NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (skill_id, version)
);
CREATE INDEX IF NOT EXISTS ix_skills_head ON skills(is_head, team_id, owner_agent_id, name);
CREATE VIRTUAL TABLE IF NOT EXISTS skills_fts USING fts5(tokens, skill_id UNINDEXED);
CREATE TABLE IF NOT EXISTS skill_buffers (
    buffer_key TEXT PRIMARY KEY, messages TEXT NOT NULL, tool_call_count INTEGER NOT NULL, byte_count INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_archives (archive_key TEXT PRIMARY KEY, messages TEXT NOT NULL, created_at_ms INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS skill_tasks (task_id TEXT PRIMARY KEY, doc TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS knowledge (
    id TEXT PRIMARY KEY, team_id TEXT NOT NULL, kind TEXT NOT NULL, created_at TEXT NOT NULL, doc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS meta_users (
    user_id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, user_type TEXT NOT NULL, status TEXT NOT NULL,
    created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_single_system_admin ON meta_users(user_type) WHERE user_type = 'system_admin';
CREATE TABLE IF NOT EXISTS meta_user_keys (
    key_id TEXT PRIMARY KEY, user_id TEXT NOT NULL, key_value TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_keys_user ON meta_user_keys(user_id, created_at);
CREATE TABLE IF NOT EXISTS meta_teams (
    team_id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS meta_team_members (
    team_id TEXT NOT NULL, user_id TEXT NOT NULL, status TEXT NOT NULL, joined_at TEXT NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (team_id, user_id)
);
CREATE TABLE IF NOT EXISTS meta_agents (
    agent_id TEXT PRIMARY KEY, team_id TEXT NOT NULL, owner_user_id TEXT NOT NULL, status TEXT NOT NULL,
    name TEXT NOT NULL, created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS meta_tasks (
    task_id TEXT PRIMARY KEY, team_id TEXT NOT NULL, creator_user_id TEXT NOT NULL, status TEXT NOT NULL,
    created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS meta_task_agents (
    task_id TEXT NOT NULL, agent_id TEXT NOT NULL, created_at TEXT NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (task_id, agent_id)
);
CREATE TABLE IF NOT EXISTS meta_participation_logs (
    id TEXT PRIMARY KEY, team_id TEXT NOT NULL, task_id TEXT NOT NULL, agent_id TEXT NOT NULL, user_id TEXT NOT NULL,
    created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS meta_assets (
    asset_id TEXT PRIMARY KEY, team_id TEXT NOT NULL, asset_type TEXT NOT NULL, status TEXT NOT NULL,
    owner_user_id TEXT NOT NULL, visibility TEXT NOT NULL, created_at TEXT NOT NULL, doc TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_assets_team ON meta_assets(team_id, created_at DESC);
CREATE TABLE IF NOT EXISTS meta_agent_bindings (
    agent_id TEXT NOT NULL, asset_id TEXT NOT NULL, priority INTEGER NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (agent_id, asset_id)
);
CREATE INDEX IF NOT EXISTS ix_bindings_asset ON meta_agent_bindings(asset_id);
CREATE TABLE IF NOT EXISTS meta_asset_acl (
    id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, subject_type TEXT NOT NULL, subject_id TEXT NOT NULL,
    permission TEXT NOT NULL, doc TEXT NOT NULL,
    UNIQUE (asset_id, subject_type, subject_id, permission)
);
CREATE TABLE IF NOT EXISTS meta_config_params (
    scope TEXT NOT NULL, user_id TEXT NOT NULL, module TEXT NOT NULL, param_name TEXT NOT NULL, param_value TEXT NOT NULL,
    PRIMARY KEY (scope, user_id, module, param_name)
);
INSERT OR IGNORE INTO meta_config_params VALUES
    ('global', '', 'quota', 'max_users_per_instance', '500'),
    ('global', '', 'quota', 'max_teams_per_instance', '100'),
    ('global', '', 'asset_type', 'skill.enabled', '1'),
    ('global', '', 'asset_type', 'llm_wiki.enabled', '1'),
    ('global', '', 'asset_type', 'code_graph.enabled', '1'),
    ('global', '', 'asset_type', 'chat_memory.enabled', '1');
"#;

const SYSTEM_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_states (
    instance TEXT NOT NULL, scope_key TEXT NOT NULL, session_id TEXT NOT NULL,
    l1_due_ms INTEGER, last_active_ms INTEGER NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (instance, scope_key, session_id)
);
CREATE INDEX IF NOT EXISTS ix_session_due ON session_states(l1_due_ms) WHERE l1_due_ms IS NOT NULL;
CREATE TABLE IF NOT EXISTS profile_states (
    instance TEXT NOT NULL, profile_key TEXT NOT NULL, l2_due_ms INTEGER, updated_ms INTEGER NOT NULL, doc TEXT NOT NULL,
    PRIMARY KEY (instance, profile_key)
);
CREATE INDEX IF NOT EXISTS ix_profile_due ON profile_states(l2_due_ms) WHERE l2_due_ms IS NOT NULL;
"#;

pub fn migrate_instance(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(INSTANCE_SCHEMA)
}

pub fn migrate_system(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SYSTEM_SCHEMA)
}
