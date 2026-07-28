//! Local progress storage.
//!
//! Everything is local and stays local: no network calls exist in this crate, by design
//! (spec 2, 17). Command history is opt-in with a retention policy, and exported transcripts
//! are redacted before they leave the application.

pub mod migrations;
pub mod redact;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use shared_types::{
    FailureCategory, LessonProgress, LessonStatus, MasteryStatus, ModuleProgress, TaskAttempt,
};
use std::path::Path;

pub use redact::redact_transcript;

/// How long terminal command history is kept (spec 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRetention {
    DoNotSave,
    LastSession,
    Days(u32),
    Forever,
}

impl HistoryRetention {
    pub fn as_setting(self) -> String {
        match self {
            Self::DoNotSave => "none".into(),
            Self::LastSession => "session".into(),
            Self::Days(d) => format!("days:{d}"),
            Self::Forever => "forever".into(),
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "session" => Self::LastSession,
            "forever" => Self::Forever,
            other => other
                .strip_prefix("days:")
                .and_then(|d| d.parse().ok())
                .map(Self::Days)
                // Unknown or absent means the privacy-preserving default.
                .unwrap_or(Self::DoNotSave),
        }
    }
}

pub struct ProgressStore {
    connection: Connection,
}

impl ProgressStore {
    /// Opens (creating if needed) the progress database and applies migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let mut connection =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        Self::configure(&connection)?;
        migrations::apply(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut connection = Connection::open_in_memory()?;
        Self::configure(&connection)?;
        migrations::apply(&mut connection)?;
        Ok(Self { connection })
    }

    fn configure(connection: &Connection) -> Result<()> {
        connection.execute_batch(
            // WAL survives an abrupt Windows shutdown far better than the rollback journal,
            // which matters because the app can be killed with the VM running.
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    /// Returns the id of the named profile, creating it on first use.
    pub fn ensure_profile(&self, name: &str, now: i64) -> Result<i64> {
        if let Some(id) = self
            .connection
            .query_row(
                "SELECT id FROM profiles WHERE name = ?1",
                params![name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            self.connection.execute(
                "UPDATE profiles SET last_active_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            return Ok(id);
        }
        self.connection.execute(
            "INSERT INTO profiles (name, created_at, last_active_at) VALUES (?1, ?2, ?2)",
            params![name, now],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Marks a lesson as started, without disturbing an existing record.
    pub fn start_lesson(&self, profile_id: i64, lesson_id: &str, now: i64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO lesson_progress
                 (profile_id, lesson_id, status, attempts, first_started_at, last_attempt_at)
             VALUES (?1, ?2, 'in-progress', 0, ?3, ?3)
             ON CONFLICT (profile_id, lesson_id) DO UPDATE SET
                 last_attempt_at = ?3,
                 status = CASE WHEN lesson_progress.status = 'not-started'
                               THEN 'in-progress' ELSE lesson_progress.status END",
            params![profile_id, lesson_id, now],
        )?;
        Ok(())
    }

    pub fn record_task_attempt(&self, attempt: &TaskAttempt) -> Result<i64> {
        let category = attempt
            .failure_category
            .map(|c| serde_json::to_value(c).map(|v| v.as_str().unwrap_or_default().to_string()))
            .transpose()?;
        self.connection.execute(
            "INSERT INTO task_attempts
                 (profile_id, lesson_id, task_id, passed, failure_category, hints_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                attempt.profile_id,
                attempt.lesson_id,
                attempt.task_id,
                attempt.passed as i64,
                category,
                attempt.hints_used,
                attempt.created_at
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Records the outcome of a whole lesson.
    ///
    /// Mastery only ever improves: a learner who revisits a mastered lesson and stumbles
    /// should not lose credit they already earned, otherwise practising is punished.
    pub fn complete_lesson(
        &self,
        profile_id: i64,
        lesson_id: &str,
        mastery: MasteryStatus,
        hints_used: u32,
        solution_revealed: bool,
        now: i64,
    ) -> Result<MasteryStatus> {
        let existing = self.lesson_progress(profile_id, lesson_id)?;
        let best = existing
            .as_ref()
            .and_then(|p| p.mastery)
            .map(|previous| previous.max(mastery))
            .unwrap_or(mastery);

        let status = if best.needs_revisiting() {
            LessonStatus::NeedsReview
        } else {
            LessonStatus::Passed
        };

        self.connection.execute(
            "INSERT INTO lesson_progress
                 (profile_id, lesson_id, status, attempts, hints_used, solution_revealed,
                  mastery, mastery_score, first_started_at, completed_at, last_attempt_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?8, ?8)
             ON CONFLICT (profile_id, lesson_id) DO UPDATE SET
                 status = ?3,
                 attempts = lesson_progress.attempts + 1,
                 hints_used = lesson_progress.hints_used + ?4,
                 solution_revealed = lesson_progress.solution_revealed OR ?5,
                 mastery = ?6,
                 mastery_score = ?7,
                 completed_at = COALESCE(lesson_progress.completed_at, ?8),
                 last_attempt_at = ?8",
            params![
                profile_id,
                lesson_id,
                status_str(status),
                hints_used,
                solution_revealed as i64,
                mastery_str(best),
                best.score(),
                now
            ],
        )?;
        Ok(best)
    }

    /// Records a failed attempt without marking the lesson complete.
    pub fn record_lesson_attempt(&self, profile_id: i64, lesson_id: &str, now: i64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO lesson_progress
                 (profile_id, lesson_id, status, attempts, first_started_at, last_attempt_at)
             VALUES (?1, ?2, 'in-progress', 1, ?3, ?3)
             ON CONFLICT (profile_id, lesson_id) DO UPDATE SET
                 attempts = lesson_progress.attempts + 1,
                 last_attempt_at = ?3",
            params![profile_id, lesson_id, now],
        )?;
        Ok(())
    }

    pub fn add_hint_used(&self, profile_id: i64, lesson_id: &str, now: i64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO lesson_progress
                 (profile_id, lesson_id, status, hints_used, first_started_at, last_attempt_at)
             VALUES (?1, ?2, 'in-progress', 1, ?3, ?3)
             ON CONFLICT (profile_id, lesson_id) DO UPDATE SET
                 hints_used = lesson_progress.hints_used + 1,
                 last_attempt_at = ?3",
            params![profile_id, lesson_id, now],
        )?;
        Ok(())
    }

    pub fn lesson_progress(
        &self,
        profile_id: i64,
        lesson_id: &str,
    ) -> Result<Option<LessonProgress>> {
        let row = self
            .connection
            .query_row(
                "SELECT status, attempts, hints_used, solution_revealed, mastery, mastery_score,
                        first_started_at, completed_at, last_attempt_at
                 FROM lesson_progress WHERE profile_id = ?1 AND lesson_id = ?2",
                params![profile_id, lesson_id],
                |row| {
                    Ok(LessonProgress {
                        profile_id,
                        lesson_id: lesson_id.to_string(),
                        status: parse_status(&row.get::<_, String>(0)?),
                        attempts: row.get(1)?,
                        hints_used: row.get(2)?,
                        solution_revealed: row.get::<_, i64>(3)? != 0,
                        mastery: row
                            .get::<_, Option<String>>(4)?
                            .as_deref()
                            .and_then(parse_mastery),
                        mastery_score: row.get(5)?,
                        first_started_at: row.get(6)?,
                        completed_at: row.get(7)?,
                        last_attempt_at: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn all_lesson_progress(&self, profile_id: i64) -> Result<Vec<LessonProgress>> {
        let mut statement = self.connection.prepare(
            "SELECT lesson_id, status, attempts, hints_used, solution_revealed, mastery,
                    mastery_score, first_started_at, completed_at, last_attempt_at
             FROM lesson_progress WHERE profile_id = ?1 ORDER BY lesson_id",
        )?;
        let rows = statement.query_map(params![profile_id], |row| {
            Ok(LessonProgress {
                profile_id,
                lesson_id: row.get(0)?,
                status: parse_status(&row.get::<_, String>(1)?),
                attempts: row.get(2)?,
                hints_used: row.get(3)?,
                solution_revealed: row.get::<_, i64>(4)? != 0,
                mastery: row
                    .get::<_, Option<String>>(5)?
                    .as_deref()
                    .and_then(parse_mastery),
                mastery_score: row.get(6)?,
                first_started_at: row.get(7)?,
                completed_at: row.get(8)?,
                last_attempt_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn upsert_module_progress(&self, progress: &ModuleProgress) -> Result<()> {
        self.connection.execute(
            "INSERT INTO module_progress
                 (profile_id, module_id, completed_lessons, total_lessons, assessment_score)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (profile_id, module_id) DO UPDATE SET
                 completed_lessons = ?3, total_lessons = ?4,
                 assessment_score = COALESCE(?5, module_progress.assessment_score)",
            params![
                progress.profile_id,
                progress.module_id,
                progress.completed_lessons,
                progress.total_lessons,
                progress.assessment_score
            ],
        )?;
        Ok(())
    }

    /// The failure categories a learner hits most, for the "weak concepts" report.
    pub fn common_failures(&self, profile_id: i64, limit: usize) -> Result<Vec<(String, u32)>> {
        let mut statement = self.connection.prepare(
            "SELECT failure_category, COUNT(*) AS hits
             FROM task_attempts
             WHERE profile_id = ?1 AND passed = 0 AND failure_category IS NOT NULL
             GROUP BY failure_category ORDER BY hits DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![profile_id, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_setting(&self, profile_id: i64, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings (profile_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (profile_id, key) DO UPDATE SET value = ?3",
            params![profile_id, key, value],
        )?;
        Ok(())
    }

    pub fn setting(&self, profile_id: i64, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM settings WHERE profile_id = ?1 AND key = ?2",
                params![profile_id, key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn history_retention(&self, profile_id: i64) -> Result<HistoryRetention> {
        Ok(self
            .setting(profile_id, "history.retention")?
            .map(|v| HistoryRetention::parse(&v))
            .unwrap_or(HistoryRetention::DoNotSave))
    }

    /// Records a command only when the retention policy allows it. Returns whether it was
    /// stored, so callers cannot accidentally assume persistence.
    pub fn record_command(
        &self,
        profile_id: i64,
        session_id: &str,
        command: &str,
        now: i64,
    ) -> Result<bool> {
        if self.history_retention(profile_id)? == HistoryRetention::DoNotSave {
            return Ok(false);
        }
        self.connection.execute(
            "INSERT INTO command_history (profile_id, session_id, command, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![profile_id, session_id, command, now],
        )?;
        Ok(true)
    }

    /// Enforces the retention policy. Called at startup and when the setting changes, so a
    /// learner who reduces retention sees old history actually disappear.
    pub fn prune_command_history(
        &self,
        profile_id: i64,
        current_session: &str,
        now: i64,
    ) -> Result<usize> {
        let removed = match self.history_retention(profile_id)? {
            HistoryRetention::DoNotSave => self.connection.execute(
                "DELETE FROM command_history WHERE profile_id = ?1",
                params![profile_id],
            )?,
            HistoryRetention::LastSession => self.connection.execute(
                "DELETE FROM command_history WHERE profile_id = ?1 AND session_id <> ?2",
                params![profile_id, current_session],
            )?,
            HistoryRetention::Days(days) => {
                let cutoff = now - (days as i64 * 86_400);
                self.connection.execute(
                    "DELETE FROM command_history WHERE profile_id = ?1 AND created_at < ?2",
                    params![profile_id, cutoff],
                )?
            }
            HistoryRetention::Forever => 0,
        };
        Ok(removed)
    }

    pub fn command_history(&self, profile_id: i64, limit: usize) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT command FROM command_history WHERE profile_id = ?1
             ORDER BY created_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![profile_id, limit as i64], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn record_snapshot(
        &self,
        profile_id: i64,
        name: &str,
        disk_path: &str,
        runtime_version: &str,
        size_bytes: u64,
        now: i64,
    ) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO snapshots
                 (profile_id, name, disk_path, created_at, runtime_version, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                profile_id,
                name,
                disk_path,
                now,
                runtime_version,
                size_bytes as i64
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn snapshots(&self, profile_id: i64) -> Result<Vec<(i64, String, String, i64)>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, disk_path, created_at FROM snapshots
             WHERE profile_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map(params![profile_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn forget_snapshot(&self, profile_id: i64, id: i64) -> Result<()> {
        self.connection.execute(
            "DELETE FROM snapshots WHERE profile_id = ?1 AND id = ?2",
            params![profile_id, id],
        )?;
        Ok(())
    }

    pub fn unlock_achievement(
        &self,
        profile_id: i64,
        achievement_id: &str,
        now: i64,
    ) -> Result<bool> {
        let changed = self.connection.execute(
            "INSERT INTO achievements (profile_id, achievement_id, unlocked_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (profile_id, achievement_id) DO NOTHING",
            params![profile_id, achievement_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn achievements(&self, profile_id: i64) -> Result<Vec<(String, i64)>> {
        let mut statement = self.connection.prepare(
            "SELECT achievement_id, unlocked_at FROM achievements
             WHERE profile_id = ?1 ORDER BY unlocked_at",
        )?;
        let rows =
            statement.query_map(params![profile_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn bump_metric(&self, profile_id: i64, key: &str, by: i64) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO metrics (profile_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (profile_id, key) DO UPDATE SET value = metrics.value + ?3",
            params![profile_id, key, by],
        )?;
        Ok(self.connection.query_row(
            "SELECT value FROM metrics WHERE profile_id = ?1 AND key = ?2",
            params![profile_id, key],
            |row| row.get(0),
        )?)
    }

    pub fn metric(&self, profile_id: i64, key: &str) -> Result<i64> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM metrics WHERE profile_id = ?1 AND key = ?2",
                params![profile_id, key],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    /// Wipes everything for a profile. Backs the "start over" control in Settings.
    pub fn reset_profile(&self, profile_id: i64) -> Result<()> {
        for table in [
            "lesson_progress",
            "task_attempts",
            "module_progress",
            "achievements",
            "command_history",
            "metrics",
            "snapshots",
        ] {
            self.connection.execute(
                &format!("DELETE FROM {table} WHERE profile_id = ?1"),
                params![profile_id],
            )?;
        }
        Ok(())
    }
}

fn status_str(status: LessonStatus) -> &'static str {
    match status {
        LessonStatus::NotStarted => "not-started",
        LessonStatus::InProgress => "in-progress",
        LessonStatus::Passed => "passed",
        LessonStatus::NeedsReview => "needs-review",
    }
}

fn parse_status(value: &str) -> LessonStatus {
    match value {
        "passed" => LessonStatus::Passed,
        "needs-review" => LessonStatus::NeedsReview,
        "in-progress" => LessonStatus::InProgress,
        _ => LessonStatus::NotStarted,
    }
}

fn mastery_str(mastery: MasteryStatus) -> &'static str {
    match mastery {
        MasteryStatus::Mastered => "mastered",
        MasteryStatus::Strong => "strong",
        MasteryStatus::Passed => "passed",
        MasteryStatus::NeedsReview => "needs-review",
        MasteryStatus::ReviewRequired => "review-required",
    }
}

fn parse_mastery(value: &str) -> Option<MasteryStatus> {
    Some(match value {
        "mastered" => MasteryStatus::Mastered,
        "strong" => MasteryStatus::Strong,
        "passed" => MasteryStatus::Passed,
        "needs-review" => MasteryStatus::NeedsReview,
        "review-required" => MasteryStatus::ReviewRequired,
        _ => return None,
    })
}

/// Renders a failure category the way it is stored, for callers building queries.
pub fn failure_category_key(category: FailureCategory) -> String {
    serde_json::to_value(category)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (ProgressStore, i64) {
        let store = ProgressStore::open_in_memory().unwrap();
        let profile = store.ensure_profile("student", 1_000).unwrap();
        (store, profile)
    }

    #[test]
    fn profiles_are_created_once_and_reused() {
        let store = ProgressStore::open_in_memory().unwrap();
        let first = store.ensure_profile("student", 1).unwrap();
        let second = store.ensure_profile("student", 2).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn lesson_progress_roundtrips() {
        let (store, profile) = store();
        store.start_lesson(profile, "m.01", 1_000).unwrap();
        let progress = store.lesson_progress(profile, "m.01").unwrap().unwrap();
        assert_eq!(progress.status, LessonStatus::InProgress);
        assert_eq!(progress.attempts, 0);
        assert_eq!(progress.first_started_at, Some(1_000));
    }

    #[test]
    fn completing_a_lesson_stores_mastery_and_score() {
        let (store, profile) = store();
        store.start_lesson(profile, "m.01", 1_000).unwrap();
        let best = store
            .complete_lesson(profile, "m.01", MasteryStatus::Strong, 1, false, 2_000)
            .unwrap();
        assert_eq!(best, MasteryStatus::Strong);

        let progress = store.lesson_progress(profile, "m.01").unwrap().unwrap();
        assert_eq!(progress.status, LessonStatus::Passed);
        assert_eq!(progress.mastery, Some(MasteryStatus::Strong));
        assert_eq!(progress.mastery_score, 85);
        assert_eq!(progress.completed_at, Some(2_000));
    }

    #[test]
    fn revisiting_a_lesson_never_downgrades_mastery() {
        let (store, profile) = store();
        store
            .complete_lesson(profile, "m.01", MasteryStatus::Mastered, 0, false, 1_000)
            .unwrap();
        let best = store
            .complete_lesson(profile, "m.01", MasteryStatus::NeedsReview, 4, true, 2_000)
            .unwrap();
        assert_eq!(
            best,
            MasteryStatus::Mastered,
            "practising must not cost credit"
        );

        let progress = store.lesson_progress(profile, "m.01").unwrap().unwrap();
        assert_eq!(progress.attempts, 2);
        assert_eq!(progress.hints_used, 4);
        // The fact that they looked at the answer is still recorded.
        assert!(progress.solution_revealed);
        // And the original completion time is preserved.
        assert_eq!(progress.completed_at, Some(1_000));
    }

    #[test]
    fn a_lesson_that_only_reaches_needs_review_is_stored_as_such() {
        let (store, profile) = store();
        store
            .complete_lesson(profile, "m.01", MasteryStatus::NeedsReview, 3, true, 1_000)
            .unwrap();
        let progress = store.lesson_progress(profile, "m.01").unwrap().unwrap();
        assert_eq!(progress.status, LessonStatus::NeedsReview);
    }

    #[test]
    fn failure_categories_are_aggregated_most_common_first() {
        let (store, profile) = store();
        for category in [
            FailureCategory::WrongPath,
            FailureCategory::WrongPath,
            FailureCategory::PermissionDenied,
        ] {
            store
                .record_task_attempt(&TaskAttempt {
                    id: None,
                    profile_id: profile,
                    lesson_id: "m.01".into(),
                    task_id: "task-1".into(),
                    passed: false,
                    failure_category: Some(category),
                    hints_used: 0,
                    created_at: 1_000,
                })
                .unwrap();
        }
        let failures = store.common_failures(profile, 5).unwrap();
        assert_eq!(failures[0], ("wrong_path".to_string(), 2));
        assert_eq!(failures[1], ("permission_denied".to_string(), 1));
    }

    #[test]
    fn passing_attempts_are_not_counted_as_failures() {
        let (store, profile) = store();
        store
            .record_task_attempt(&TaskAttempt {
                id: None,
                profile_id: profile,
                lesson_id: "m.01".into(),
                task_id: "task-1".into(),
                passed: true,
                failure_category: None,
                hints_used: 0,
                created_at: 1_000,
            })
            .unwrap();
        assert!(store.common_failures(profile, 5).unwrap().is_empty());
    }

    #[test]
    fn command_history_is_off_by_default() {
        let (store, profile) = store();
        assert_eq!(
            store.history_retention(profile).unwrap(),
            HistoryRetention::DoNotSave
        );
        let stored = store
            .record_command(profile, "s1", "ls -la", 1_000)
            .unwrap();
        assert!(!stored, "history must not be saved without opting in");
        assert!(store.command_history(profile, 10).unwrap().is_empty());
    }

    #[test]
    fn opting_in_records_history() {
        let (store, profile) = store();
        store
            .set_setting(profile, "history.retention", "forever")
            .unwrap();
        assert!(store
            .record_command(profile, "s1", "ls -la", 1_000)
            .unwrap());
        assert_eq!(store.command_history(profile, 10).unwrap(), vec!["ls -la"]);
    }

    #[test]
    fn reducing_retention_actually_deletes_old_history() {
        let (store, profile) = store();
        store
            .set_setting(profile, "history.retention", "forever")
            .unwrap();
        store
            .record_command(profile, "old-session", "secret command", 1_000)
            .unwrap();

        store
            .set_setting(profile, "history.retention", "session")
            .unwrap();
        let removed = store
            .prune_command_history(profile, "new-session", 2_000)
            .unwrap();
        assert_eq!(removed, 1);
        assert!(store.command_history(profile, 10).unwrap().is_empty());
    }

    #[test]
    fn day_based_retention_keeps_recent_and_drops_old() {
        let (store, profile) = store();
        store
            .set_setting(profile, "history.retention", "days:30")
            .unwrap();
        let now = 30 * 86_400 * 2;
        store.record_command(profile, "s", "old", 1).unwrap();
        store
            .record_command(profile, "s", "recent", now - 100)
            .unwrap();
        store.prune_command_history(profile, "s", now).unwrap();
        assert_eq!(store.command_history(profile, 10).unwrap(), vec!["recent"]);
    }

    #[test]
    fn retention_setting_parses_and_renders_symmetrically() {
        for retention in [
            HistoryRetention::DoNotSave,
            HistoryRetention::LastSession,
            HistoryRetention::Days(30),
            HistoryRetention::Forever,
        ] {
            assert_eq!(HistoryRetention::parse(&retention.as_setting()), retention);
        }
        // Anything unrecognised falls back to the private default.
        assert_eq!(HistoryRetention::parse("wat"), HistoryRetention::DoNotSave);
    }

    #[test]
    fn achievements_unlock_once() {
        let (store, profile) = store();
        assert!(store
            .unlock_achievement(profile, "first-lesson", 1)
            .unwrap());
        assert!(!store
            .unlock_achievement(profile, "first-lesson", 2)
            .unwrap());
        assert_eq!(store.achievements(profile).unwrap().len(), 1);
    }

    #[test]
    fn metrics_accumulate() {
        let (store, profile) = store();
        assert_eq!(store.metric(profile, "practice.seconds").unwrap(), 0);
        store.bump_metric(profile, "practice.seconds", 60).unwrap();
        assert_eq!(
            store.bump_metric(profile, "practice.seconds", 30).unwrap(),
            90
        );
    }

    #[test]
    fn resetting_a_profile_clears_progress_but_keeps_the_profile() {
        let (store, profile) = store();
        store
            .complete_lesson(profile, "m.01", MasteryStatus::Mastered, 0, false, 1)
            .unwrap();
        store.reset_profile(profile).unwrap();
        assert!(store.all_lesson_progress(profile).unwrap().is_empty());
        assert_eq!(store.ensure_profile("student", 2).unwrap(), profile);
    }

    #[test]
    fn store_survives_reopening_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("progress.db");
        {
            let store = ProgressStore::open(&path).unwrap();
            let profile = store.ensure_profile("student", 1).unwrap();
            store
                .complete_lesson(profile, "m.01", MasteryStatus::Strong, 1, false, 1)
                .unwrap();
        }
        let store = ProgressStore::open(&path).unwrap();
        let profile = store.ensure_profile("student", 2).unwrap();
        assert_eq!(
            store
                .lesson_progress(profile, "m.01")
                .unwrap()
                .unwrap()
                .mastery,
            Some(MasteryStatus::Strong)
        );
    }
}
