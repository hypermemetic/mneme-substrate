//! SQLite-backed index over program directories.
//!
//! Programs are still primarily on disk (manifest.json, artifact.json, trace.jsonl,
//! sessions/) — that's what makes `git log` over `programs/` useful as audit
//! history. SQLite holds a queryable mirror of the manifest metadata so callers
//! can ask things like "what programs are running right now?" or "show me the
//! last 20 forecast updates" without scanning the filesystem.
//!
//! The DB is the index; the filesystem is the truth. If the two ever disagree,
//! re-derive from the filesystem.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

use super::program::{ProgramId, ProgramStatus};

/// Errors raised by `MnemeStorage`.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("invalid timestamp in row")]
    InvalidTimestamp,
}

/// One row from the programs table — the manifest metadata that's worth
/// indexing for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramRow {
    pub program_id: String,
    pub parent_program_id: Option<String>,
    pub entry_skill: String,
    pub status: ProgramStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub substrate_version: String,
    pub mneme_version: String,
    pub depth: u8,
}

/// SQLite-backed storage for the programs index.
#[derive(Clone)]
pub struct MnemeStorage {
    pool: SqlitePool,
}

impl std::fmt::Debug for MnemeStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MnemeStorage").finish_non_exhaustive()
    }
}

impl MnemeStorage {
    /// Open or create the database at the given path.
    pub async fn open(db_path: PathBuf) -> Result<Self, StorageError> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;
        let storage = Self { pool };
        storage.run_migrations().await?;
        Ok(storage)
    }

    /// In-memory database — used for tests; doesn't persist.
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let storage = Self { pool };
        storage.run_migrations().await?;
        Ok(storage)
    }

    async fn run_migrations(&self) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mneme_programs (
                program_id TEXT PRIMARY KEY,
                parent_program_id TEXT,
                entry_skill TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                substrate_version TEXT NOT NULL,
                mneme_version TEXT NOT NULL,
                depth INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_mneme_programs_status ON mneme_programs(status);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_mneme_programs_started_at ON mneme_programs(started_at);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_mneme_programs_parent ON mneme_programs(parent_program_id);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_mneme_programs_entry_skill ON mneme_programs(entry_skill);",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// INSERT a new running program row. Called from `Program::open`.
    pub async fn insert_program(
        &self,
        program_id: &ProgramId,
        parent_program_id: Option<&ProgramId>,
        entry_skill: &str,
        substrate_version: &str,
        mneme_version: &str,
        depth: u8,
    ) -> Result<(), StorageError> {
        let now = Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO mneme_programs (program_id, parent_program_id, entry_skill, status, started_at, substrate_version, mneme_version, depth) VALUES (?, ?, ?, 'running', ?, ?, ?, ?);"
        )
        .bind(program_id.to_string())
        .bind(parent_program_id.map(ProgramId::to_string))
        .bind(entry_skill)
        .bind(now)
        .bind(substrate_version)
        .bind(mneme_version)
        .bind(depth as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// UPDATE the status + finished_at of a program. Called from
    /// `Program::close_completed` / `close_failed`.
    pub async fn update_status(
        &self,
        program_id: &ProgramId,
        status: ProgramStatus,
    ) -> Result<(), StorageError> {
        let now = Utc::now().timestamp_millis();
        let status_str = match status {
            ProgramStatus::Running => "running",
            ProgramStatus::Completed => "completed",
            ProgramStatus::Failed => "failed",
        };
        sqlx::query(
            "UPDATE mneme_programs SET status = ?, finished_at = ? WHERE program_id = ?;",
        )
        .bind(status_str)
        .bind(now)
        .bind(program_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Look up one program by id.
    pub async fn get(&self, program_id: &ProgramId) -> Result<Option<ProgramRow>, StorageError> {
        let row = sqlx::query(
            "SELECT program_id, parent_program_id, entry_skill, status, started_at, finished_at, substrate_version, mneme_version, depth FROM mneme_programs WHERE program_id = ?;"
        )
        .bind(program_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_program).transpose()
    }

    /// List programs, newest first. Optional status filter.
    pub async fn list(
        &self,
        status: Option<ProgramStatus>,
        limit: u32,
    ) -> Result<Vec<ProgramRow>, StorageError> {
        let rows = match status {
            Some(s) => {
                let s_str = match s {
                    ProgramStatus::Running => "running",
                    ProgramStatus::Completed => "completed",
                    ProgramStatus::Failed => "failed",
                };
                sqlx::query(
                    "SELECT program_id, parent_program_id, entry_skill, status, started_at, finished_at, substrate_version, mneme_version, depth FROM mneme_programs WHERE status = ? ORDER BY started_at DESC LIMIT ?;"
                )
                .bind(s_str)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT program_id, parent_program_id, entry_skill, status, started_at, finished_at, substrate_version, mneme_version, depth FROM mneme_programs ORDER BY started_at DESC LIMIT ?;"
                )
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
        };
        rows.into_iter().map(row_to_program).collect()
    }
}

fn row_to_program(row: sqlx::sqlite::SqliteRow) -> Result<ProgramRow, StorageError> {
    let status_str: String = row.get("status");
    let status = match status_str.as_str() {
        "running" => ProgramStatus::Running,
        "completed" => ProgramStatus::Completed,
        "failed" => ProgramStatus::Failed,
        _ => ProgramStatus::Running,
    };
    let started_ms: i64 = row.get("started_at");
    let finished_ms: Option<i64> = row.get("finished_at");
    let started_at = Utc
        .timestamp_millis_opt(started_ms)
        .single()
        .ok_or(StorageError::InvalidTimestamp)?;
    let finished_at = finished_ms
        .map(|ms| Utc.timestamp_millis_opt(ms).single().ok_or(StorageError::InvalidTimestamp))
        .transpose()?;
    let depth: i64 = row.get("depth");
    Ok(ProgramRow {
        program_id: row.get("program_id"),
        parent_program_id: row.get("parent_program_id"),
        entry_skill: row.get("entry_skill"),
        status,
        started_at,
        finished_at,
        substrate_version: row.get("substrate_version"),
        mneme_version: row.get("mneme_version"),
        depth: depth as u8,
    })
}

/// Convenience: an `Arc<MnemeStorage>` is what `MnemeContext` holds.
pub type SharedStorage = Arc<MnemeStorage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_in_memory_and_migrate() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        assert!(s.list(None, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn insert_then_get() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        let id = ProgramId::new();
        s.insert_program(&id, None, "forecast.update", "0.6.3", "0.1.0", 0)
            .await
            .unwrap();
        let row = s.get(&id).await.unwrap().expect("present");
        assert_eq!(row.entry_skill, "forecast.update");
        assert_eq!(row.status, ProgramStatus::Running);
        assert!(row.finished_at.is_none());
    }

    #[tokio::test]
    async fn update_status_to_completed() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        let id = ProgramId::new();
        s.insert_program(&id, None, "x", "v", "v", 0).await.unwrap();
        s.update_status(&id, ProgramStatus::Completed).await.unwrap();
        let row = s.get(&id).await.unwrap().unwrap();
        assert_eq!(row.status, ProgramStatus::Completed);
        assert!(row.finished_at.is_some());
    }

    #[tokio::test]
    async fn list_filters_by_status_and_orders_newest_first() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        let id1 = ProgramId::new();
        let id2 = ProgramId::new();
        let id3 = ProgramId::new();
        s.insert_program(&id1, None, "a", "v", "v", 0).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        s.insert_program(&id2, None, "b", "v", "v", 0).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        s.insert_program(&id3, None, "c", "v", "v", 0).await.unwrap();

        s.update_status(&id1, ProgramStatus::Completed).await.unwrap();

        let all = s.list(None, 10).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].entry_skill, "c"); // newest first

        let completed = s.list(Some(ProgramStatus::Completed), 10).await.unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].entry_skill, "a");
    }

    #[tokio::test]
    async fn parent_program_id_persisted() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        let parent = ProgramId::new();
        let child = ProgramId::new();
        s.insert_program(&parent, None, "outer", "v", "v", 0).await.unwrap();
        s.insert_program(&child, Some(&parent), "inner", "v", "v", 1).await.unwrap();

        let row = s.get(&child).await.unwrap().unwrap();
        assert_eq!(row.parent_program_id.as_deref(), Some(parent.to_string().as_str()));
        assert_eq!(row.depth, 1);
    }

    #[tokio::test]
    async fn get_unknown_returns_none() {
        let s = MnemeStorage::open_in_memory().await.unwrap();
        assert!(s.get(&ProgramId::new()).await.unwrap().is_none());
    }
}
