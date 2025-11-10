use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{named_params, params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::task;
use tracing::debug;
use worktree_core::workflows::feature::{FeatureStatus, StartSummary, TeardownSummary};

const DB_NAME: &str = "agent.db";

#[derive(Clone)]
pub struct AgentState {
    conn: Arc<Mutex<Connection>>,
}

impl AgentState {
    pub fn initialize(state_dir: &Path) -> Result<Self> {
        let db_path = state_dir.join(DB_NAME);
        let connection = Connection::open(&db_path).with_context(|| {
            format!(
                "Failed to open agent state database at {}",
                db_path.display()
            )
        })?;

        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;

        Self::run_migrations(&connection)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(connection)),
        })
    }

    pub async fn record_feature_start(&self, summary: &StartSummary) -> Result<()> {
        let record = WorktreeRecord::from_start(summary);
        let payload = json!({
            "event": "feature_start",
            "work_feature": &record.work_feature,
            "branch_name": &record.branch_name,
            "worktree_path": &record.worktree_path,
            "metadata": &record.metadata,
        });

        self.execute(move |conn| {
            conn.execute(
                r#"
                INSERT INTO worktrees (work_feature, branch_name, worktree_path, status, metadata, updated_at)
                VALUES (:work_feature, :branch_name, :worktree_path, :status, :metadata, :updated_at)
                ON CONFLICT(work_feature) DO UPDATE SET
                    branch_name = excluded.branch_name,
                    worktree_path = excluded.worktree_path,
                    status = excluded.status,
                    metadata = excluded.metadata,
                    updated_at = excluded.updated_at
                "#,
                params![
                    record.work_feature,
                    record.branch_name,
                    record.worktree_path,
                    record.status,
                    serde_json::to_string(&record.metadata)?,
                    record.updated_at.to_rfc3339(),
                ],
            )?;

            conn.execute(
                r#"
                INSERT INTO events (event_type, payload, queued_at)
                VALUES (?1, ?2, ?3)
                "#,
                params![
                    "feature_start",
                    serde_json::to_string(&payload)?,
                    record.updated_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn record_feature_teardown(&self, summary: &TeardownSummary) -> Result<()> {
        let record = WorktreeRecord::from_teardown(summary);
        let payload = json!({
            "event": "feature_teardown",
            "work_feature": &record.work_feature,
            "branch_name": &record.branch_name,
            "worktree_removed": summary.worktree_removed,
            "branch_deleted": summary.branch_deleted,
            "warnings": summary.warnings,
        });

        self.execute(move |conn| {
            conn.execute(
                r#"
                INSERT INTO worktrees (work_feature, branch_name, worktree_path, status, metadata, updated_at)
                VALUES (:work_feature, :branch_name, :worktree_path, :status, :metadata, :updated_at)
                ON CONFLICT(work_feature) DO UPDATE SET
                    status = excluded.status,
                    metadata = excluded.metadata,
                    updated_at = excluded.updated_at
                "#,
                params![
                    record.work_feature,
                    record.branch_name,
                    record.worktree_path,
                    record.status,
                    serde_json::to_string(&record.metadata)?,
                    record.updated_at.to_rfc3339(),
                ],
            )?;

            conn.execute(
                r#"
                INSERT INTO events (event_type, payload, queued_at)
                VALUES (?1, ?2, ?3)
                "#,
                params![
                    "feature_teardown",
                    serde_json::to_string(&payload)?,
                    record.updated_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn enqueue_heartbeat(&self, workspace_root: &Path) -> Result<()> {
        let snapshot = self.snapshot_worktrees().await?;
        let now = Utc::now();
        let payload = json!({
            "event": "heartbeat",
            "workspace_root": workspace_root.display().to_string(),
            "timestamp": now.to_rfc3339(),
            "worktrees": snapshot,
        });

        self.execute(move |conn| {
            conn.execute(
                r#"
                INSERT INTO events (event_type, payload, queued_at)
                VALUES (?1, ?2, ?3)
                "#,
                params![
                    "heartbeat",
                    serde_json::to_string(&payload)?,
                    now.to_rfc3339()
                ],
            )?;
            conn.execute(
                r#"
                INSERT INTO heartbeats (id, last_sent_at) VALUES (1, ?1)
                ON CONFLICT(id) DO UPDATE SET last_sent_at = excluded.last_sent_at
                "#,
                params![now.to_rfc3339()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn snapshot_worktrees(&self) -> Result<Vec<StoredWorktree>> {
        self.execute(move |conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT work_feature, branch_name, worktree_path, status, metadata, updated_at
                FROM worktrees
                ORDER BY updated_at DESC
                "#,
            )?;

            let rows = stmt
                .query_map([], |row| {
                    let metadata_raw: String = row.get(4)?;
                    let metadata: serde_json::Value =
                        serde_json::from_str(&metadata_raw).unwrap_or_default();
                    Ok(StoredWorktree {
                        work_feature: row.get(0)?,
                        branch_name: row.get(1)?,
                        worktree_path: row.get(2)?,
                        status: row.get(3)?,
                        metadata,
                        updated_at: row.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok(rows)
        })
        .await
    }

    pub async fn dequeue_events(&self, limit: usize) -> Result<Vec<PendingEvent>> {
        self.execute(move |conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, event_type, payload, queued_at
                FROM events
                WHERE delivered_at IS NULL
                ORDER BY id ASC
                LIMIT ?1
                "#,
            )?;

            let events = stmt
                .query_map([limit as i64], |row| {
                    let payload: Value =
                        serde_json::from_str(row.get::<_, String>(2)?.as_str()).unwrap_or_default();
                    let queued_at = DateTime::parse_from_rfc3339(row.get::<_, String>(3)?.as_str())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    Ok(PendingEvent {
                        id: row.get(0)?,
                        event_type: row.get(1)?,
                        payload,
                        queued_at,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok(events)
        })
        .await
    }

    pub async fn mark_events_delivered(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let ids = ids.to_owned();
        self.execute(move |conn| {
            let tx = conn.transaction()?;
            {
                let delivered_at = Utc::now().to_rfc3339();
                let mut stmt = tx.prepare(
                    r#"
                    UPDATE events
                    SET delivered_at = :delivered_at
                    WHERE id = :id
                    "#,
                )?;
                for id in &ids {
                    stmt.execute(named_params! {
                        ":delivered_at": &delivered_at,
                        ":id": id,
                    })?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn next_batch_id(&self) -> Result<i64> {
        self.execute(move |conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT next_batch_id
                FROM control_plane_status
                WHERE id = 1
                "#,
            )?;
            let current: i64 = stmt.query_row([], |row| row.get(0))?;
            let next = current.saturating_add(1);
            let now = Utc::now().to_rfc3339();
            conn.execute(
                r#"
                UPDATE control_plane_status
                SET next_batch_id = :next_batch_id,
                    updated_at = :updated_at
                WHERE id = 1
                "#,
                named_params! {":next_batch_id": next, ":updated_at": now},
            )?;
            Ok(current)
        })
        .await
    }

    pub async fn update_control_plane_ack(&self, ack_event_id: i64) -> Result<()> {
        self.execute(move |conn| {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                r#"
                UPDATE control_plane_status
                SET last_ack_event_id = CASE
                        WHEN last_ack_event_id IS NULL THEN :ack_event_id
                        WHEN last_ack_event_id < :ack_event_id THEN :ack_event_id
                        ELSE last_ack_event_id
                    END,
                    updated_at = :updated_at
                WHERE id = 1
                "#,
                named_params! {":ack_event_id": ack_event_id, ":updated_at": now},
            )?;
            Ok(())
        })
        .await
    }

    #[allow(dead_code)]
    pub async fn control_plane_status(&self) -> Result<ControlPlaneStatus> {
        self.execute(move |conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT last_ack_event_id, next_batch_id
                FROM control_plane_status
                WHERE id = 1
                "#,
            )?;
            let status = stmt.query_row([], |row| {
                Ok(ControlPlaneStatus {
                    last_ack_event_id: row.get(0)?,
                    next_batch_id: row.get(1)?,
                })
            })?;
            Ok(status)
        })
        .await
    }

    async fn execute<F, T>(&self, action: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        task::spawn_blocking(move || {
            let mut guard = conn.lock().expect("AgentState connection mutex poisoned");
            action(&mut guard)
        })
        .await
        .context("AgentState task join error")?
    }

    fn run_migrations(connection: &Connection) -> Result<()> {
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS worktrees (
                work_feature TEXT PRIMARY KEY,
                branch_name TEXT NOT NULL,
                worktree_path TEXT NOT NULL,
                status TEXT NOT NULL,
                metadata TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                queued_at TEXT NOT NULL,
                delivered_at TEXT
            );

            CREATE TABLE IF NOT EXISTS heartbeats (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_sent_at TEXT
            );

            CREATE TABLE IF NOT EXISTS control_plane_status (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_ack_event_id INTEGER,
                next_batch_id INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );

            INSERT OR IGNORE INTO control_plane_status (id, last_ack_event_id, next_batch_id, updated_at)
            VALUES (1, NULL, 1, datetime('now'));
            "#,
        )?;

        debug!("Agent state migrations applied");
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct WorktreeRecord {
    work_feature: String,
    branch_name: String,
    worktree_path: String,
    status: String,
    metadata: serde_json::Value,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingEvent {
    pub id: i64,
    pub event_type: String,
    pub payload: Value,
    pub queued_at: DateTime<Utc>,
}

impl WorktreeRecord {
    fn from_start(summary: &StartSummary) -> Self {
        Self {
            work_feature: summary.work_feature.clone(),
            branch_name: summary.branch_name.clone(),
            worktree_path: summary.worktree_path.display().to_string(),
            status: FeatureStatus::Active.to_string(),
            metadata: json!({
                "feature_url": summary.feature_url.clone(),
                "compose_project_name": summary.compose_project_name.clone(),
                "env_path": summary.env_path.as_ref().map(|p| p.display().to_string()),
                "color": summary.color.clone(),
                "mode": summary.mode.to_string(),
                "prompt_seed": summary.prompt_seed.clone(),
            }),
            updated_at: summary.generated_at,
        }
    }

    fn from_teardown(summary: &TeardownSummary) -> Self {
        Self {
            work_feature: summary.work_feature.clone(),
            branch_name: summary.branch_name.clone(),
            worktree_path: String::new(),
            status: FeatureStatus::Removed.to_string(),
            metadata: json!({
                "warnings": summary.warnings.clone(),
                "branch_deleted": summary.branch_deleted,
                "worktree_removed": summary.worktree_removed,
            }),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StoredWorktree {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub status: String,
    pub metadata: serde_json::Value,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct ControlPlaneStatus {
    pub last_ack_event_id: Option<i64>,
    pub next_batch_id: i64,
}

#[cfg(test)]
mod tests {
    use super::AgentState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn next_batch_id_increments_monotonically() {
        let dir = tempdir().unwrap();
        let state = AgentState::initialize(dir.path()).unwrap();

        let first = state.next_batch_id().await.unwrap();
        let second = state.next_batch_id().await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
    }

    #[tokio::test]
    async fn control_plane_ack_persists_high_watermark() {
        let dir = tempdir().unwrap();
        let state = AgentState::initialize(dir.path()).unwrap();

        state.update_control_plane_ack(5).await.unwrap();
        let status = state.control_plane_status().await.unwrap();
        assert_eq!(status.last_ack_event_id, Some(5));

        // Lower acknowledgements should be ignored.
        state.update_control_plane_ack(3).await.unwrap();
        let status = state.control_plane_status().await.unwrap();
        assert_eq!(status.last_ack_event_id, Some(5));

        state.update_control_plane_ack(42).await.unwrap();
        let status = state.control_plane_status().await.unwrap();
        assert_eq!(status.last_ack_event_id, Some(42));
    }
}
