use rusqlite::{params, Connection, OptionalExtension};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_SQL: &str = include_str!("../../specs/v1/sqlite-schema.sql");

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RelocationRecord {
    pub relocation_id: String,
    pub app_id: String,
    pub tier: String,
    pub mode: String,
    pub source_path: String,
    pub target_root: String,
    pub target_path: String,
    pub backup_path: Option<String>,
    pub state: String,
    pub health_state: String,
    pub last_error_code: Option<String>,
    pub trace_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewRelocationRecord {
    pub relocation_id: String,
    pub app_id: String,
    pub tier: String,
    pub mode: String,
    pub source_path: String,
    pub target_root: String,
    pub target_path: String,
    pub backup_path: Option<String>,
    pub state: String,
    pub health_state: String,
    pub last_error_code: Option<String>,
    pub trace_id: String,
    pub source_size_bytes: i64,
    pub target_size_bytes: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewOperationLogEntry {
    pub log_id: String,
    pub relocation_id: String,
    pub trace_id: String,
    pub stage: String,
    pub step: String,
    pub status: String,
    pub error_code: Option<String>,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
    pub details_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewHealthSnapshot {
    pub snapshot_id: String,
    pub relocation_id: String,
    pub state: String,
    pub check_code: String,
    pub details_json: String,
    pub observed_at: String,
}

#[derive(Debug, Clone)]
pub struct LatestHealthSnapshot {
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub check_code: String,
    pub details_json: String,
    pub observed_at: String,
}

#[derive(Debug, Clone)]
pub struct HealthEventRecord {
    pub snapshot_id: String,
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub check_code: String,
    pub details_json: String,
    pub observed_at: String,
}

#[derive(Debug, Clone)]
pub struct OperationLogRecord {
    pub log_id: String,
    pub relocation_id: String,
    pub trace_id: String,
    pub stage: String,
    pub step: String,
    pub status: String,
    pub error_code: Option<String>,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
    pub details_json: String,
    pub created_at: String,
}

impl Database {
    pub fn init(base_dir: PathBuf) -> Result<Self, Box<dyn Error>> {
        fs::create_dir_all(&base_dir)?;
        let db_path = base_dir.join("disk-relocator.sqlite3");
        let connection = Connection::open(&db_path)?;
        connection.execute_batch(SCHEMA_SQL)?;

        Ok(Self { path: db_path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        Connection::open(&self.path)
    }

    pub fn insert_relocation(&self, row: &NewRelocationRecord) -> rusqlite::Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO relocations (
              relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
              state, health_state, last_error_code, trace_id, source_size_bytes, target_size_bytes,
              metadata_version, created_at, updated_at, completed_at
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
              ?9, ?10, ?11, ?12, ?13, ?14,
              1, ?15, ?16, ?17
            )
            "#,
            params![
                row.relocation_id,
                row.app_id,
                row.tier,
                row.mode,
                row.source_path,
                row.target_root,
                row.target_path,
                row.backup_path,
                row.state,
                row.health_state,
                row.last_error_code,
                row.trace_id,
                row.source_size_bytes,
                row.target_size_bytes,
                row.created_at,
                row.updated_at,
                row.completed_at,
            ],
        )?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_relocation_state(
        &self,
        relocation_id: &str,
        state: &str,
        health_state: &str,
        trace_id: &str,
        last_error_code: Option<&str>,
        updated_at: &str,
        completed_at: Option<&str>,
    ) -> rusqlite::Result<usize> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            UPDATE relocations
            SET state = ?2,
                health_state = ?3,
                trace_id = ?4,
                last_error_code = ?5,
                updated_at = ?6,
                completed_at = ?7
            WHERE relocation_id = ?1
            "#,
            params![
                relocation_id,
                state,
                health_state,
                trace_id,
                last_error_code,
                updated_at,
                completed_at
            ],
        )
    }

    pub fn update_relocation_sizes(
        &self,
        relocation_id: &str,
        source_size_bytes: i64,
        target_size_bytes: i64,
        updated_at: &str,
    ) -> rusqlite::Result<usize> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            UPDATE relocations
            SET source_size_bytes = ?2,
                target_size_bytes = ?3,
                updated_at = ?4
            WHERE relocation_id = ?1
            "#,
            params![
                relocation_id,
                source_size_bytes,
                target_size_bytes,
                updated_at
            ],
        )
    }

    pub fn get_relocation(
        &self,
        relocation_id: &str,
    ) -> rusqlite::Result<Option<RelocationRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
                   state, health_state, last_error_code, trace_id, created_at, updated_at
            FROM relocations
            WHERE relocation_id = ?1
            LIMIT 1
            "#,
        )?;

        stmt.query_row(params![relocation_id], row_to_relocation)
            .optional()
    }

    pub fn list_relocations(&self) -> rusqlite::Result<Vec<RelocationRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
                   state, health_state, last_error_code, trace_id, created_at, updated_at
            FROM relocations
            ORDER BY datetime(updated_at) DESC, rowid DESC
            "#,
        )?;
        let rows = stmt.query_map([], row_to_relocation)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_unfinished_relocations(&self) -> rusqlite::Result<Vec<RelocationRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
                   state, health_state, last_error_code, trace_id, created_at, updated_at
            FROM relocations
            WHERE state IN (
              'PRECHECKING',
              'BOOTSTRAP_INIT',
              'COPYING',
              'VERIFYING',
              'SWITCHING',
              'POSTCHECKING',
              'FAILED_NEEDS_ROLLBACK',
              'ROLLING_BACK'
            )
            ORDER BY datetime(updated_at) ASC, rowid ASC
            "#,
        )?;
        let rows = stmt.query_map([], row_to_relocation)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_operation_logs(
        &self,
        relocation_id: Option<&str>,
        trace_id: Option<&str>,
    ) -> rusqlite::Result<Vec<OperationLogRecord>> {
        let connection = self.connect()?;
        let mut stmt = match (relocation_id, trace_id) {
            (Some(_), Some(_)) => connection.prepare(
                r#"
                SELECT log_id, relocation_id, trace_id, stage, step, status, error_code,
                       duration_ms, message, details_json, created_at
                FROM operation_logs
                WHERE relocation_id = ?1 AND trace_id = ?2
                ORDER BY datetime(created_at) ASC, rowid ASC
                "#,
            )?,
            (Some(_), None) => connection.prepare(
                r#"
                SELECT log_id, relocation_id, trace_id, stage, step, status, error_code,
                       duration_ms, message, details_json, created_at
                FROM operation_logs
                WHERE relocation_id = ?1
                ORDER BY datetime(created_at) ASC, rowid ASC
                "#,
            )?,
            (None, Some(_)) => connection.prepare(
                r#"
                SELECT log_id, relocation_id, trace_id, stage, step, status, error_code,
                       duration_ms, message, details_json, created_at
                FROM operation_logs
                WHERE trace_id = ?1
                ORDER BY datetime(created_at) ASC, rowid ASC
                "#,
            )?,
            (None, None) => connection.prepare(
                r#"
                SELECT log_id, relocation_id, trace_id, stage, step, status, error_code,
                       duration_ms, message, details_json, created_at
                FROM operation_logs
                ORDER BY datetime(created_at) ASC, rowid ASC
                "#,
            )?,
        };

        let rows = match (relocation_id, trace_id) {
            (Some(relocation), Some(trace)) => {
                stmt.query_map(params![relocation, trace], row_to_operation_log)?
            }
            (Some(relocation), None) => {
                stmt.query_map(params![relocation], row_to_operation_log)?
            }
            (None, Some(trace)) => stmt.query_map(params![trace], row_to_operation_log)?,
            (None, None) => stmt.query_map([], row_to_operation_log)?,
        };

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_health_monitoring_relocations(&self) -> rusqlite::Result<Vec<RelocationRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            WITH ranked AS (
              SELECT relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
                     state, health_state, last_error_code, trace_id, created_at, updated_at,
                     ROW_NUMBER() OVER (
                       PARTITION BY app_id
                       ORDER BY datetime(updated_at) DESC, rowid DESC
                     ) AS rank_latest
              FROM relocations
              WHERE state IN ('HEALTHY', 'DEGRADED', 'BROKEN')
            )
            SELECT relocation_id, app_id, tier, mode, source_path, target_root, target_path, backup_path,
                   state, health_state, last_error_code, trace_id, created_at, updated_at
            FROM ranked
            WHERE rank_latest = 1
            ORDER BY datetime(updated_at) DESC
            "#,
        )?;
        let rows = stmt.query_map([], row_to_relocation)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn update_relocation_health(
        &self,
        relocation_id: &str,
        state: &str,
        health_state: &str,
        trace_id: &str,
        last_error_code: Option<&str>,
        updated_at: &str,
    ) -> rusqlite::Result<usize> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            UPDATE relocations
            SET state = ?2,
                health_state = ?3,
                trace_id = ?4,
                last_error_code = ?5,
                updated_at = ?6
            WHERE relocation_id = ?1
            "#,
            params![
                relocation_id,
                state,
                health_state,
                trace_id,
                last_error_code,
                updated_at
            ],
        )
    }

    pub fn insert_operation_log(&self, row: &NewOperationLogEntry) -> rusqlite::Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO operation_logs (
              log_id, relocation_id, trace_id, stage, step, status, error_code,
              duration_ms, message, details_json, created_at
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5, ?6, ?7,
              ?8, ?9, ?10, ?11
            )
            "#,
            params![
                row.log_id,
                row.relocation_id,
                row.trace_id,
                row.stage,
                row.step,
                row.status,
                row.error_code,
                row.duration_ms,
                row.message,
                row.details_json,
                row.created_at
            ],
        )?;

        Ok(())
    }

    pub fn insert_health_snapshot(&self, row: &NewHealthSnapshot) -> rusqlite::Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO health_snapshots (
              snapshot_id, relocation_id, state, check_code, details_json, observed_at
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5, ?6
            )
            "#,
            params![
                row.snapshot_id,
                row.relocation_id,
                row.state,
                row.check_code,
                row.details_json,
                row.observed_at
            ],
        )?;
        Ok(())
    }

    pub fn list_latest_health_snapshots(&self) -> rusqlite::Result<Vec<LatestHealthSnapshot>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT hs.relocation_id, r.app_id, hs.state, hs.check_code, hs.details_json, hs.observed_at
            FROM health_snapshots hs
            INNER JOIN (
              SELECT relocation_id, MAX(observed_at) AS max_observed_at
              FROM health_snapshots
              GROUP BY relocation_id
            ) latest
              ON latest.relocation_id = hs.relocation_id
             AND latest.max_observed_at = hs.observed_at
            INNER JOIN relocations r ON r.relocation_id = hs.relocation_id
            ORDER BY datetime(hs.observed_at) DESC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LatestHealthSnapshot {
                relocation_id: row.get(0)?,
                app_id: row.get(1)?,
                state: row.get(2)?,
                check_code: row.get(3)?,
                details_json: row.get(4)?,
                observed_at: row.get(5)?,
            })
        })?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row?);
        }
        Ok(snapshots)
    }

    pub fn list_health_events(&self, limit: usize) -> rusqlite::Result<Vec<HealthEventRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT hs.snapshot_id, hs.relocation_id, r.app_id, hs.state, hs.check_code, hs.details_json, hs.observed_at
            FROM health_snapshots hs
            INNER JOIN relocations r ON r.relocation_id = hs.relocation_id
            ORDER BY datetime(hs.observed_at) DESC, hs.rowid DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(HealthEventRecord {
                snapshot_id: row.get(0)?,
                relocation_id: row.get(1)?,
                app_id: row.get(2)?,
                state: row.get(3)?,
                check_code: row.get(4)?,
                details_json: row.get(5)?,
                observed_at: row.get(6)?,
            })
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }
}

fn row_to_relocation(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelocationRecord> {
    Ok(RelocationRecord {
        relocation_id: row.get(0)?,
        app_id: row.get(1)?,
        tier: row.get(2)?,
        mode: row.get(3)?,
        source_path: row.get(4)?,
        target_root: row.get(5)?,
        target_path: row.get(6)?,
        backup_path: row.get(7)?,
        state: row.get(8)?,
        health_state: row.get(9)?,
        last_error_code: row.get(10)?,
        trace_id: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn row_to_operation_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationLogRecord> {
    Ok(OperationLogRecord {
        log_id: row.get(0)?,
        relocation_id: row.get(1)?,
        trace_id: row.get(2)?,
        stage: row.get(3)?,
        step: row.get(4)?,
        status: row.get(5)?,
        error_code: row.get(6)?,
        duration_ms: row.get(7)?,
        message: row.get(8)?,
        details_json: row.get(9)?,
        created_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn database_can_persist_and_query_relocation_flow() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");
        let created_at = "2026-03-05T10:00:00Z".to_string();

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_test_001".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/Library/Application Support/Telegram Desktop".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Telegram Desktop".to_string(),
            backup_path: Some(
                "/Users/test/Library/Application Support/Telegram Desktop.bak".to_string(),
            ),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_test_001".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at.clone(),
            completed_at: Some(created_at.clone()),
        })
        .expect("insert relocation");

        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_test_001".to_string(),
            relocation_id: "reloc_test_001".to_string(),
            trace_id: "tr_test_001".to_string(),
            stage: "migration".to_string(),
            step: "metadata_commit".to_string(),
            status: "succeeded".to_string(),
            error_code: None,
            duration_ms: Some(12),
            message: Some("ok".to_string()),
            details_json: "{}".to_string(),
            created_at: created_at.clone(),
        })
        .expect("insert operation log");

        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_test_001".to_string(),
            relocation_id: "reloc_test_001".to_string(),
            state: "healthy".to_string(),
            check_code: "HEALTH_METADATA_ONLY_OK".to_string(),
            details_json: "{\"message\":\"ok\"}".to_string(),
            observed_at: created_at.clone(),
        })
        .expect("insert health snapshot");

        let row = db
            .get_relocation("reloc_test_001")
            .expect("query relocation")
            .expect("row should exist");
        assert_eq!(row.app_id, "telegram-desktop");
        assert_eq!(row.state, "HEALTHY");

        let list = db.list_relocations().expect("list relocations");
        assert_eq!(list.len(), 1);

        let health = db
            .list_latest_health_snapshots()
            .expect("query latest health snapshots");
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].check_code, "HEALTH_METADATA_ONLY_OK");

        let events = db.list_health_events(10).expect("query health events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].snapshot_id, "snap_test_001");

        let updated_at = "2026-03-05T10:10:00Z".to_string();
        db.update_relocation_state(
            "reloc_test_001",
            "ROLLED_BACK",
            "healthy",
            "tr_test_rollback",
            None,
            &updated_at,
            Some(&updated_at),
        )
        .expect("update relocation state");

        let updated = db
            .get_relocation("reloc_test_001")
            .expect("query updated relocation")
            .expect("row should exist");
        assert_eq!(updated.state, "ROLLED_BACK");
        assert_eq!(updated.trace_id, "tr_test_rollback");

        let logs_for_relocation = db
            .list_operation_logs(Some("reloc_test_001"), None)
            .expect("list logs by relocation");
        assert_eq!(logs_for_relocation.len(), 1);
        assert_eq!(logs_for_relocation[0].step, "metadata_commit");

        let logs_for_trace = db
            .list_operation_logs(None, Some("tr_test_001"))
            .expect("list logs by trace");
        assert_eq!(logs_for_trace.len(), 1);
        assert_eq!(logs_for_trace[0].relocation_id, "reloc_test_001");

        db.update_relocation_state(
            "reloc_test_001",
            "HEALTHY",
            "healthy",
            "tr_test_healthy",
            None,
            &updated_at,
            Some(&updated_at),
        )
        .expect("reset to healthy for monitor list");

        let monitor_rows = db
            .list_health_monitoring_relocations()
            .expect("list monitor relocations");
        assert_eq!(monitor_rows.len(), 1);
        assert_eq!(monitor_rows[0].relocation_id, "reloc_test_001");

        let monitor_updated_at = "2026-03-05T10:20:00Z".to_string();
        db.update_relocation_health(
            "reloc_test_001",
            "DEGRADED",
            "degraded",
            "tr_test_health",
            Some("HEALTH_TARGET_READONLY"),
            &monitor_updated_at,
        )
        .expect("update relocation health");
        let monitored = db
            .get_relocation("reloc_test_001")
            .expect("query monitored row")
            .expect("row should exist");
        assert_eq!(monitored.state, "DEGRADED");
        assert_eq!(monitored.health_state, "degraded");
        assert_eq!(
            monitored.last_error_code.as_deref(),
            Some("HEALTH_TARGET_READONLY")
        );
        assert_eq!(monitored.trace_id, "tr_test_health");
    }

    #[test]
    fn operation_logs_support_combined_filter_and_time_order() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");
        let created_at = "2026-03-05T10:00:00Z".to_string();

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_log_001".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/Library/Containers/com.tencent.xinWeChat".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChat".to_string(),
            backup_path: Some(
                "/Users/test/Library/Containers/com.tencent.xinWeChat.bak".to_string(),
            ),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_log_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            completed_at: None,
        })
        .expect("insert relocation");

        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_log_001".to_string(),
            relocation_id: "reloc_log_001".to_string(),
            trace_id: "tr_log_1".to_string(),
            stage: "migration".to_string(),
            step: "copy_to_temp".to_string(),
            status: "started".to_string(),
            error_code: None,
            duration_ms: Some(1),
            message: Some("copy started".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:00:01Z".to_string(),
        })
        .expect("insert log 1");

        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_log_002".to_string(),
            relocation_id: "reloc_log_001".to_string(),
            trace_id: "tr_log_1".to_string(),
            stage: "migration".to_string(),
            step: "copy_to_temp".to_string(),
            status: "failed".to_string(),
            error_code: Some("MIGRATE_COPY_FAILED".to_string()),
            duration_ms: Some(22),
            message: Some("copy failed".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:00:03Z".to_string(),
        })
        .expect("insert log 2");

        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_log_003".to_string(),
            relocation_id: "reloc_log_001".to_string(),
            trace_id: "tr_log_2".to_string(),
            stage: "rollback".to_string(),
            step: "state_restore".to_string(),
            status: "succeeded".to_string(),
            error_code: None,
            duration_ms: Some(8),
            message: Some("rollback done".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:01:00Z".to_string(),
        })
        .expect("insert log 3");

        let all = db
            .list_operation_logs(Some("reloc_log_001"), None)
            .expect("query all logs by relocation");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].log_id, "log_log_001");
        assert_eq!(all[1].log_id, "log_log_002");
        assert_eq!(all[2].log_id, "log_log_003");

        let filtered = db
            .list_operation_logs(Some("reloc_log_001"), Some("tr_log_1"))
            .expect("query logs by relocation + trace");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].log_id, "log_log_001");
        assert_eq!(filtered[1].log_id, "log_log_002");
    }

    #[test]
    fn health_monitoring_list_filters_active_states_and_orders_desc() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_active_old".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-old".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChatOld".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_1".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:01:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert active old");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_inactive".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-inactive".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Telegram".to_string(),
            backup_path: None,
            state: "ROLLED_BACK".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_2".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:02:00Z".to_string(),
            updated_at: "2026-03-05T10:03:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert inactive");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_active_new".to_string(),
            app_id: "xcode-derived-data".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-new".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Xcode".to_string(),
            backup_path: None,
            state: "DEGRADED".to_string(),
            health_state: "degraded".to_string(),
            last_error_code: Some("HEALTH_TARGET_READONLY".to_string()),
            trace_id: "tr_seed_3".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:04:00Z".to_string(),
            updated_at: "2026-03-05T10:05:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert active new");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_active_wechat_latest".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-latest".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChatLatest".to_string(),
            backup_path: None,
            state: "BROKEN".to_string(),
            health_state: "broken".to_string(),
            last_error_code: Some("HEALTH_SYMLINK_MISSING".to_string()),
            trace_id: "tr_seed_4".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:06:00Z".to_string(),
            updated_at: "2026-03-05T10:07:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert active latest wechat");

        let rows = db
            .list_health_monitoring_relocations()
            .expect("list monitoring rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].relocation_id, "reloc_active_wechat_latest");
        assert_eq!(rows[1].relocation_id, "reloc_active_new");
    }

    #[test]
    fn latest_health_snapshots_returns_latest_per_relocation() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");

        let created_at = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_snap_001".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-1".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChat".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_snap_1".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at.clone(),
            completed_at: None,
        })
        .expect("insert relocation 1");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_snap_002".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-2".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Telegram".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_snap_2".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            completed_at: None,
        })
        .expect("insert relocation 2");

        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_001_old".to_string(),
            relocation_id: "reloc_snap_001".to_string(),
            state: "degraded".to_string(),
            check_code: "HEALTH_DISK_OFFLINE".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:01:00Z".to_string(),
        })
        .expect("insert old snapshot");
        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_001_new".to_string(),
            relocation_id: "reloc_snap_001".to_string(),
            state: "healthy".to_string(),
            check_code: "HEALTH_RW_PROBE_OK".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:02:00Z".to_string(),
        })
        .expect("insert new snapshot");
        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_002_only".to_string(),
            relocation_id: "reloc_snap_002".to_string(),
            state: "healthy".to_string(),
            check_code: "HEALTH_RW_PROBE_OK".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:01:30Z".to_string(),
        })
        .expect("insert second snapshot");

        let latest = db
            .list_latest_health_snapshots()
            .expect("list latest snapshots");
        assert_eq!(latest.len(), 2);

        let first = latest
            .iter()
            .find(|row| row.relocation_id == "reloc_snap_001")
            .expect("first relocation latest row");
        assert_eq!(first.check_code, "HEALTH_RW_PROBE_OK");
        assert_eq!(first.observed_at, "2026-03-05T10:02:00Z");

        let second = latest
            .iter()
            .find(|row| row.relocation_id == "reloc_snap_002")
            .expect("second relocation latest row");
        assert_eq!(second.check_code, "HEALTH_RW_PROBE_OK");
        assert_eq!(second.observed_at, "2026-03-05T10:01:30Z");
    }

    #[test]
    fn unfinished_relocations_are_filtered_and_ordered_by_updated_time() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_unfinished_old".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-old".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChatOld".to_string(),
            backup_path: None,
            state: "PRECHECKING".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_old".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:01:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert unfinished old");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_finished".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-finished".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Telegram".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_finished".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:02:00Z".to_string(),
            updated_at: "2026-03-05T10:03:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert finished row");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_unfinished_new".to_string(),
            app_id: "xcode-derived-data".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source-new".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/Xcode".to_string(),
            backup_path: None,
            state: "SWITCHING".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_new".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:04:00Z".to_string(),
            updated_at: "2026-03-05T10:05:00Z".to_string(),
            completed_at: None,
        })
        .expect("insert unfinished new");

        let rows = db
            .list_unfinished_relocations()
            .expect("list unfinished rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].relocation_id, "reloc_unfinished_old");
        assert_eq!(rows[1].relocation_id, "reloc_unfinished_new");
    }

    #[test]
    fn health_events_respect_limit_and_latest_first_order() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");

        let created_at = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_limit".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChat".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_health_limit".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            completed_at: None,
        })
        .expect("insert relocation");

        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_limit_1".to_string(),
            relocation_id: "reloc_health_limit".to_string(),
            state: "healthy".to_string(),
            check_code: "HEALTH_1".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:00:01Z".to_string(),
        })
        .expect("insert snapshot 1");
        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_limit_2".to_string(),
            relocation_id: "reloc_health_limit".to_string(),
            state: "degraded".to_string(),
            check_code: "HEALTH_2".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:00:02Z".to_string(),
        })
        .expect("insert snapshot 2");
        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: "snap_limit_3".to_string(),
            relocation_id: "reloc_health_limit".to_string(),
            state: "broken".to_string(),
            check_code: "HEALTH_3".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:00:03Z".to_string(),
        })
        .expect("insert snapshot 3");

        let events = db.list_health_events(2).expect("list health events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].snapshot_id, "snap_limit_3");
        assert_eq!(events[1].snapshot_id, "snap_limit_2");
    }

    #[test]
    fn operation_logs_trace_only_filter_returns_time_sorted_rows() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::init(dir.path().to_path_buf()).expect("init database");

        let created_at = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_trace_only".to_string(),
            app_id: "wechat-non-mas".to_string(),
            tier: "experimental".to_string(),
            mode: "migrate".to_string(),
            source_path: "/Users/test/source".to_string(),
            target_root: "/Volumes/TestSSD".to_string(),
            target_path: "/Volumes/TestSSD/AppData/WeChat".to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            completed_at: None,
        })
        .expect("insert relocation");

        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_trace_1".to_string(),
            relocation_id: "reloc_trace_only".to_string(),
            trace_id: "tr_keep".to_string(),
            stage: "migration".to_string(),
            step: "copy_to_temp".to_string(),
            status: "started".to_string(),
            error_code: None,
            duration_ms: Some(1),
            message: Some("copy started".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:00:01Z".to_string(),
        })
        .expect("insert trace log 1");
        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_trace_2".to_string(),
            relocation_id: "reloc_trace_only".to_string(),
            trace_id: "tr_drop".to_string(),
            stage: "migration".to_string(),
            step: "copy_to_temp".to_string(),
            status: "failed".to_string(),
            error_code: Some("MIGRATE_COPY_FAILED".to_string()),
            duration_ms: Some(2),
            message: Some("copy failed".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:00:02Z".to_string(),
        })
        .expect("insert trace log 2");
        db.insert_operation_log(&NewOperationLogEntry {
            log_id: "log_trace_3".to_string(),
            relocation_id: "reloc_trace_only".to_string(),
            trace_id: "tr_keep".to_string(),
            stage: "rollback".to_string(),
            step: "state_restore".to_string(),
            status: "succeeded".to_string(),
            error_code: None,
            duration_ms: Some(3),
            message: Some("rollback done".to_string()),
            details_json: "{}".to_string(),
            created_at: "2026-03-05T10:00:03Z".to_string(),
        })
        .expect("insert trace log 3");

        let rows = db
            .list_operation_logs(None, Some("tr_keep"))
            .expect("list trace-only logs");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].log_id, "log_trace_1");
        assert_eq!(rows[1].log_id, "log_trace_3");
    }
}
