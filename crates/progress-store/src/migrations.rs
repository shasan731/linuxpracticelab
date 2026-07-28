//! Schema migrations.
//!
//! Migrations are append-only and applied inside a transaction, tracked by `user_version`.
//! An upgrade must never lose a learner's progress, so a released migration is never edited
//! in place — a new one is added instead.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

/// Each entry is (target version, SQL). Index 0 creates the schema from nothing.
const MIGRATIONS: &[(i64, &str)] = &[(
    1,
    r#"
CREATE TABLE profiles (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT    NOT NULL UNIQUE,
    created_at      INTEGER NOT NULL,
    last_active_at  INTEGER NOT NULL
);

CREATE TABLE lesson_progress (
    profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    lesson_id         TEXT    NOT NULL,
    status            TEXT    NOT NULL CHECK (status IN ('not-started','in-progress','passed','needs-review')),
    attempts          INTEGER NOT NULL DEFAULT 0,
    hints_used        INTEGER NOT NULL DEFAULT 0,
    solution_revealed INTEGER NOT NULL DEFAULT 0,
    mastery           TEXT,
    mastery_score     INTEGER NOT NULL DEFAULT 0,
    first_started_at  INTEGER,
    completed_at      INTEGER,
    last_attempt_at   INTEGER,
    PRIMARY KEY (profile_id, lesson_id)
);

CREATE TABLE task_attempts (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id       INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    lesson_id        TEXT    NOT NULL,
    task_id          TEXT    NOT NULL,
    passed           INTEGER NOT NULL,
    failure_category TEXT,
    hints_used       INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL
);

CREATE INDEX idx_task_attempts_lesson ON task_attempts(profile_id, lesson_id);
CREATE INDEX idx_task_attempts_category ON task_attempts(profile_id, failure_category);

CREATE TABLE module_progress (
    profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    module_id         TEXT    NOT NULL,
    completed_lessons INTEGER NOT NULL DEFAULT 0,
    total_lessons     INTEGER NOT NULL DEFAULT 0,
    assessment_score  INTEGER,
    PRIMARY KEY (profile_id, module_id)
);

CREATE TABLE snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    disk_path       TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    runtime_version TEXT    NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE settings (
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    value      TEXT    NOT NULL,
    PRIMARY KEY (profile_id, key)
);

CREATE TABLE achievements (
    profile_id     INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    achievement_id TEXT    NOT NULL,
    unlocked_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, achievement_id)
);

-- Command history is opt-in and expires. The retention policy lives in settings; this table
-- only ever holds what the current policy allows (spec 17).
CREATE TABLE command_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    session_id  TEXT    NOT NULL,
    command     TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_command_history_age ON command_history(profile_id, created_at);

-- Aggregate counters for the Progress screen, kept as rows rather than columns so new
-- metrics do not need a migration.
CREATE TABLE metrics (
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    value      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, key)
);
"#,
)];

pub fn latest_version() -> i64 {
    MIGRATIONS.last().map(|(v, _)| *v).unwrap_or(0)
}

pub fn apply(connection: &mut Connection) -> Result<i64> {
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let target = latest_version();

    if current > target {
        // A newer build wrote this database. Refusing is safer than silently mangling it.
        bail!(
            "this progress database was created by a newer version of Linux Practice Lab \
             (schema {current}, this build understands {target}). Update the application, or \
             start a new profile."
        );
    }

    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let transaction = connection.transaction()?;
        transaction
            .execute_batch(sql)
            .with_context(|| format!("migration {version} failed"))?;
        // PRAGMA does not accept bound parameters.
        transaction.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        transaction.commit()?;
        tracing::info!("applied progress database migration {version}");
    }

    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrating_an_empty_database_creates_every_table() {
        let mut connection = Connection::open_in_memory().unwrap();
        assert_eq!(apply(&mut connection).unwrap(), latest_version());

        let tables: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for expected in [
            "achievements",
            "command_history",
            "lesson_progress",
            "metrics",
            "module_progress",
            "profiles",
            "settings",
            "snapshots",
            "task_attempts",
        ] {
            assert!(tables.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        apply(&mut connection).unwrap();
        // Running again must not fail with "table already exists".
        apply(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, latest_version());
    }

    #[test]
    fn a_future_schema_is_refused_rather_than_corrupted() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("PRAGMA user_version = 9999")
            .unwrap();
        let err = apply(&mut connection).unwrap_err().to_string();
        assert!(err.contains("newer version"), "{err}");
    }

    #[test]
    fn status_values_are_constrained() {
        let mut connection = Connection::open_in_memory().unwrap();
        apply(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO profiles (id, name, created_at, last_active_at) VALUES (1,'a',0,0)",
                [],
            )
            .unwrap();
        let bad = connection.execute(
            "INSERT INTO lesson_progress (profile_id, lesson_id, status) VALUES (1,'l','nonsense')",
            [],
        );
        assert!(
            bad.is_err(),
            "the status CHECK constraint should reject this"
        );
    }
}
