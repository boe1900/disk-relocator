use crate::bootstrap::{rollback_bootstrap_switch, BootstrapSwitchOutcome};
use crate::db::{Database, NewHealthSnapshot, NewOperationLogEntry, RelocationRecord};
use crate::migration::rollback_migration_paths;
use chrono::Utc;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct RecoverySummary {
    pub total: usize,
    pub healthy: usize,
    pub rolled_back: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
struct RecoveryError {
    code: &'static str,
    message: String,
    retryable: bool,
    details: serde_json::Value,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn new_log_id() -> String {
    format!("log_{}", Uuid::new_v4().simple())
}

fn new_snapshot_id() -> String {
    format!("snap_{}", Uuid::new_v4().simple())
}

fn recovery_error(
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
    details: serde_json::Value,
) -> RecoveryError {
    RecoveryError {
        code,
        message: message.into(),
        retryable,
        details,
    }
}

fn insert_log(
    db: &Database,
    relocation_id: &str,
    trace_id: &str,
    step: &str,
    status: &str,
    error_code: Option<&str>,
    message: &str,
    details: serde_json::Value,
) -> Result<(), String> {
    db.insert_operation_log(&NewOperationLogEntry {
        log_id: new_log_id(),
        relocation_id: relocation_id.to_string(),
        trace_id: trace_id.to_string(),
        stage: "rollback".to_string(),
        step: step.to_string(),
        status: status.to_string(),
        error_code: error_code.map(str::to_string),
        duration_ms: Some(0),
        message: Some(message.to_string()),
        details_json: details.to_string(),
        created_at: now_iso(),
    })
    .map_err(|err| format!("insert recovery log failed: {err}"))
}

fn update_final_state(
    db: &Database,
    relocation_id: &str,
    trace_id: &str,
    state: &str,
    health_state: &str,
    last_error_code: Option<&str>,
) -> Result<(), String> {
    let completed_at = now_iso();
    db.update_relocation_state(
        relocation_id,
        state,
        health_state,
        trace_id,
        last_error_code,
        &completed_at,
        Some(&completed_at),
    )
    .map_err(|err| format!("update recovery state failed: {err}"))?;
    Ok(())
}

fn insert_health_snapshot(
    db: &Database,
    relocation_id: &str,
    state: &str,
    check_code: &str,
    message: &str,
) -> Result<(), String> {
    db.insert_health_snapshot(&NewHealthSnapshot {
        snapshot_id: new_snapshot_id(),
        relocation_id: relocation_id.to_string(),
        state: state.to_string(),
        check_code: check_code.to_string(),
        details_json: json!({ "message": message }).to_string(),
        observed_at: now_iso(),
    })
    .map_err(|err| format!("insert recovery health snapshot failed: {err}"))
}

fn recover_bootstrap(record: &RelocationRecord) -> Result<(String, String, String), RecoveryError> {
    let source = Path::new(&record.source_path);
    let target = Path::new(&record.target_path);

    match fs::symlink_metadata(source) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if target.exists() {
                return Ok((
                    "HEALTHY".to_string(),
                    "healthy".to_string(),
                    "Recovered unfinished bootstrap into healthy state.".to_string(),
                ));
            }
        }
        _ => {}
    }

    let source_removed = !source.exists();
    let target_created = target.exists();
    let outcome = BootstrapSwitchOutcome {
        source_placeholder_removed: source_removed,
        target_dir_created: target_created,
    };

    rollback_bootstrap_switch(source, target, &outcome).map_err(|err| {
        recovery_error(
            err.code,
            format!("bootstrap rollback recovery failed: {}", err.message),
            err.retryable,
            err.details,
        )
    })?;

    if !source.exists() {
        fs::create_dir_all(source).map_err(|err| {
            recovery_error(
                "ROLLBACK_RESTORE_BACKUP_FAILED",
                "failed to recreate source path in bootstrap recovery.",
                false,
                json!({ "source_path": record.source_path, "error": err.to_string() }),
            )
        })?;
    }

    Ok((
        "ROLLED_BACK".to_string(),
        "healthy".to_string(),
        "Recovered unfinished bootstrap by rollback cleanup.".to_string(),
    ))
}

fn recover_migrate(record: &RelocationRecord) -> Result<(String, String, String), RecoveryError> {
    let source = Path::new(&record.source_path);
    let target = Path::new(&record.target_path);
    let backup_path = record
        .backup_path
        .clone()
        .unwrap_or_else(|| format!("{}.bak", record.source_path));
    let backup = PathBuf::from(backup_path);
    let temp = PathBuf::from(format!(
        "{}.tmp.{}",
        record.target_path, record.relocation_id
    ));

    let source_is_symlink = fs::symlink_metadata(source)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);

    if source_is_symlink && target.exists() {
        return Ok((
            "HEALTHY".to_string(),
            "healthy".to_string(),
            "Recovered unfinished migrate into healthy state.".to_string(),
        ));
    }

    if source.exists() && target.exists() && !backup.exists() && !temp.exists() {
        return Err(recovery_error(
            "HEALTH_METADATA_DRIFT",
            "ambiguous migrate recovery state: source and target both exist without backup.",
            false,
            json!({
                "source_path": record.source_path,
                "target_path": record.target_path
            }),
        ));
    }

    rollback_migration_paths(source, &temp, target, &backup).map_err(|err| {
        recovery_error(
            err.code,
            format!("migrate rollback recovery failed: {}", err.message),
            err.retryable,
            err.details,
        )
    })?;

    if !source.exists() {
        return Err(recovery_error(
            "ROLLBACK_RESTORE_BACKUP_FAILED",
            "source path still missing after migrate rollback recovery.",
            false,
            json!({
                "source_path": record.source_path,
                "target_path": record.target_path,
                "backup_path": backup
            }),
        ));
    }

    Ok((
        "ROLLED_BACK".to_string(),
        "healthy".to_string(),
        "Recovered unfinished migrate by rollback cleanup.".to_string(),
    ))
}

pub fn recover_unfinished_relocations(db: &Database) -> Result<RecoverySummary, String> {
    let records = db
        .list_unfinished_relocations()
        .map_err(|err| format!("list unfinished relocations failed: {err}"))?;

    let mut summary = RecoverySummary {
        total: records.len(),
        ..RecoverySummary::default()
    };

    for record in records {
        let trace_id = record.trace_id.clone();
        insert_log(
            db,
            &record.relocation_id,
            &trace_id,
            "recovery_reconcile",
            "started",
            None,
            "startup recovery started",
            json!({
                "mode": record.mode,
                "state": record.state,
                "source_path": record.source_path,
                "target_path": record.target_path
            }),
        )?;

        let result = match record.mode.as_str() {
            "bootstrap" => recover_bootstrap(&record),
            "migrate" => recover_migrate(&record),
            _ => Err(recovery_error(
                "ROLLBACK_METADATA_RESTORE_FAILED",
                "unsupported relocation mode in recovery.",
                false,
                json!({ "mode": record.mode }),
            )),
        };

        match result {
            Ok((state, health_state, message)) => {
                update_final_state(
                    db,
                    &record.relocation_id,
                    &trace_id,
                    &state,
                    &health_state,
                    None,
                )?;
                insert_log(
                    db,
                    &record.relocation_id,
                    &trace_id,
                    "recovery_reconcile",
                    "succeeded",
                    None,
                    &message,
                    json!({ "state": state, "health_state": health_state }),
                )?;

                if state == "HEALTHY" {
                    summary.healthy += 1;
                    insert_health_snapshot(
                        db,
                        &record.relocation_id,
                        "healthy",
                        "HEALTH_RECOVERY_HEALTHY",
                        "Startup recovery resolved to healthy state.",
                    )?;
                } else {
                    summary.rolled_back += 1;
                    insert_health_snapshot(
                        db,
                        &record.relocation_id,
                        "healthy",
                        "HEALTH_RECOVERY_ROLLED_BACK",
                        "Startup recovery resolved by rollback.",
                    )?;
                }
            }
            Err(err) => {
                update_final_state(
                    db,
                    &record.relocation_id,
                    &trace_id,
                    "ROLLBACK_FAILED",
                    "broken",
                    Some(err.code),
                )?;
                insert_log(
                    db,
                    &record.relocation_id,
                    &trace_id,
                    "recovery_reconcile",
                    "failed",
                    Some(err.code),
                    &err.message,
                    err.details,
                )?;
                insert_health_snapshot(
                    db,
                    &record.relocation_id,
                    "broken",
                    "HEALTH_RECOVERY_FAILED",
                    "Startup recovery failed; manual intervention required.",
                )?;
                let _ = err.retryable;
                summary.failed += 1;
            }
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewRelocationRecord;
    use tempfile::tempdir;

    #[test]
    fn recovery_marks_symlinked_migrate_as_healthy() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        let backup = dir.path().join("source.bak");
        fs::create_dir_all(&target).expect("create target");
        fs::create_dir_all(&backup).expect("create backup");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_recover_healthy".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(backup.to_string_lossy().to_string()),
            state: "POSTCHECKING".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        })
        .expect("insert relocation");

        let summary = recover_unfinished_relocations(&db).expect("recover");
        assert_eq!(summary.total, 1);
        assert_eq!(summary.healthy, 1);

        let row = db
            .get_relocation("reloc_recover_healthy")
            .expect("query row")
            .expect("row exists");
        assert_eq!(row.state, "HEALTHY");
        assert_eq!(row.health_state, "healthy");
    }

    #[test]
    fn recovery_rolls_back_partial_migrate_state() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        let backup = dir.path().join("source.bak");

        fs::create_dir_all(&backup).expect("create backup");
        fs::write(backup.join("payload.txt"), b"hello").expect("write backup");
        fs::create_dir_all(&target).expect("create target");
        fs::write(target.join("temp.txt"), b"partial").expect("write target");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_recover_rollback".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(backup.to_string_lossy().to_string()),
            state: "SWITCHING".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        })
        .expect("insert relocation");

        let summary = recover_unfinished_relocations(&db).expect("recover");
        assert_eq!(summary.total, 1);
        assert_eq!(summary.rolled_back, 1);

        let row = db
            .get_relocation("reloc_recover_rollback")
            .expect("query row")
            .expect("row exists");
        assert_eq!(row.state, "ROLLED_BACK");
        assert!(source.exists());
        assert!(!target.exists());
    }
}
