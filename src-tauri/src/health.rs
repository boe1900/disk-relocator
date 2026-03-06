use crate::db::{Database, NewHealthSnapshot, NewOperationLogEntry, RelocationRecord};
use crate::models::{HealthCheck, HealthStatus};
use chrono::Utc;
use notify::{RecursiveMode, Watcher};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const HEALTH_POLL_INTERVAL_SECS: u64 = 30;
const VOLUMES_MOUNT_ROOT: &str = "/Volumes";

#[derive(Debug, Clone)]
struct HealthEvaluation {
    state: String,
    check_code: String,
    message: String,
    details: serde_json::Value,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn new_snapshot_id() -> String {
    format!("snap_{}", Uuid::new_v4().simple())
}

fn new_log_id() -> String {
    format!("log_{}", Uuid::new_v4().simple())
}

fn new_monitor_trace_id(trigger: &str) -> String {
    format!("tr_health_{}_{}", trigger, Uuid::new_v4().simple())
}

fn relocation_state_from_health(health_state: &str) -> &'static str {
    match health_state {
        "healthy" => "HEALTHY",
        "degraded" => "DEGRADED",
        "broken" => "BROKEN",
        _ => "DEGRADED",
    }
}

fn evaluate_health(record: &RelocationRecord) -> HealthEvaluation {
    let source = Path::new(&record.source_path);
    let target = Path::new(&record.target_path);
    let target_root = Path::new(&record.target_root);

    let source_meta = match fs::symlink_metadata(source) {
        Ok(meta) => meta,
        Err(err) => {
            let details = json!({
                "source_path": record.source_path,
                "error": err.to_string()
            });
            return HealthEvaluation {
                state: "broken".to_string(),
                check_code: "HEALTH_SYMLINK_MISSING".to_string(),
                message: "source path symlink missing.".to_string(),
                details,
            };
        }
    };

    if !source_meta.file_type().is_symlink() {
        return HealthEvaluation {
            state: "broken".to_string(),
            check_code: "HEALTH_SYMLINK_MISSING".to_string(),
            message: "source path is not a symlink.".to_string(),
            details: json!({ "source_path": record.source_path }),
        };
    }

    let linked_target = match fs::read_link(source) {
        Ok(path) => path,
        Err(err) => {
            return HealthEvaluation {
                state: "broken".to_string(),
                check_code: "HEALTH_SYMLINK_MISSING".to_string(),
                message: "failed to resolve source symlink.".to_string(),
                details: json!({ "source_path": record.source_path, "error": err.to_string() }),
            };
        }
    };

    let linked_target_abs = if linked_target.is_absolute() {
        linked_target.clone()
    } else {
        source
            .parent()
            .map(|parent| parent.join(&linked_target))
            .unwrap_or_else(|| linked_target.clone())
    };

    if linked_target_abs != target {
        return HealthEvaluation {
            state: "degraded".to_string(),
            check_code: "HEALTH_METADATA_DRIFT".to_string(),
            message: "symlink target differs from relocation metadata.".to_string(),
            details: json!({
                "source_path": record.source_path,
                "linked_target": linked_target_abs,
                "expected_target": record.target_path
            }),
        };
    }

    if !target_root.is_dir() {
        return HealthEvaluation {
            state: "degraded".to_string(),
            check_code: "HEALTH_DISK_OFFLINE".to_string(),
            message: "target disk appears offline or not mounted.".to_string(),
            details: json!({ "target_root": record.target_root }),
        };
    }

    let target_meta = match fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(err) => {
            return HealthEvaluation {
                state: "broken".to_string(),
                check_code: "HEALTH_TARGET_MISSING".to_string(),
                message: "target path is missing.".to_string(),
                details: json!({ "target_path": record.target_path, "error": err.to_string() }),
            };
        }
    };

    if !target_meta.is_dir() {
        return HealthEvaluation {
            state: "broken".to_string(),
            check_code: "HEALTH_TARGET_MISSING".to_string(),
            message: "target path is not a directory.".to_string(),
            details: json!({ "target_path": record.target_path }),
        };
    }

    let probe_file = target.join(format!(".disk-relocator-health-probe-{}", Uuid::new_v4()));
    let mut probe = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_file)
    {
        Ok(file) => file,
        Err(err) => {
            let (code, message) = if err.kind() == std::io::ErrorKind::PermissionDenied {
                (
                    "HEALTH_TARGET_READONLY",
                    "target path is read-only for health probe.",
                )
            } else {
                (
                    "HEALTH_RW_PROBE_FAILED",
                    "failed to create health probe file.",
                )
            };
            return HealthEvaluation {
                state: "degraded".to_string(),
                check_code: code.to_string(),
                message: message.to_string(),
                details: json!({ "target_path": record.target_path, "error": err.to_string() }),
            };
        }
    };

    if let Err(err) = probe.write_all(b"ok") {
        return HealthEvaluation {
            state: "degraded".to_string(),
            check_code: "HEALTH_RW_PROBE_FAILED".to_string(),
            message: "failed to write health probe file.".to_string(),
            details: json!({ "target_path": record.target_path, "error": err.to_string() }),
        };
    }

    if let Err(err) = fs::remove_file(&probe_file) {
        return HealthEvaluation {
            state: "degraded".to_string(),
            check_code: "HEALTH_RW_PROBE_FAILED".to_string(),
            message: "failed to clean health probe file.".to_string(),
            details: json!({ "probe_file": probe_file, "error": err.to_string() }),
        };
    }

    HealthEvaluation {
        state: "healthy".to_string(),
        check_code: "HEALTH_RW_PROBE_OK".to_string(),
        message: "symlink and rw probe are healthy.".to_string(),
        details: json!({
            "source_path": record.source_path,
            "target_path": record.target_path
        }),
    }
}

pub fn run_health_check(
    db: &Database,
    trace_id: &str,
    write_operation_logs: bool,
) -> Result<Vec<HealthStatus>, String> {
    let records = db
        .list_health_monitoring_relocations()
        .map_err(|err| format!("list monitor relocations failed: {err}"))?;

    let mut statuses = Vec::with_capacity(records.len());
    for record in records {
        let observed_at = now_iso();
        let evaluation = evaluate_health(&record);
        let relocation_state = relocation_state_from_health(&evaluation.state);
        let is_ok = evaluation.state == "healthy";
        let status_marker = if is_ok { "succeeded" } else { "failed" };
        let error_code = if is_ok {
            None
        } else {
            Some(evaluation.check_code.as_str())
        };

        db.update_relocation_health(
            &record.relocation_id,
            relocation_state,
            &evaluation.state,
            trace_id,
            error_code,
            &observed_at,
        )
        .map_err(|err| format!("update relocation health failed: {err}"))?;

        db.insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: new_snapshot_id(),
            relocation_id: record.relocation_id.clone(),
            state: evaluation.state.clone(),
            check_code: evaluation.check_code.clone(),
            details_json: evaluation.details.to_string(),
            observed_at: observed_at.clone(),
        })
        .map_err(|err| format!("insert health snapshot failed: {err}"))?;

        if write_operation_logs {
            db.insert_operation_log(&NewOperationLogEntry {
                log_id: new_log_id(),
                relocation_id: record.relocation_id.clone(),
                trace_id: trace_id.to_string(),
                stage: "health".to_string(),
                step: "evaluate_relocation".to_string(),
                status: status_marker.to_string(),
                error_code: error_code.map(str::to_string),
                duration_ms: Some(0),
                message: Some(evaluation.message.clone()),
                details_json: evaluation.details.to_string(),
                created_at: observed_at.clone(),
            })
            .map_err(|err| format!("insert health operation log failed: {err}"))?;
        }

        statuses.push(HealthStatus {
            relocation_id: record.relocation_id.clone(),
            app_id: record.app_id.clone(),
            state: evaluation.state.clone(),
            checks: vec![HealthCheck {
                code: evaluation.check_code.clone(),
                ok: is_ok,
                message: evaluation.message.clone(),
            }],
            observed_at,
        });
    }

    Ok(statuses)
}

fn run_background_cycle(db: &Database, trigger: &str) {
    let trace_id = new_monitor_trace_id(trigger);
    if let Err(err) = run_health_check(db, &trace_id, false) {
        eprintln!("[health-monitor] run failed ({trigger}): {err}");
    }
}

pub fn spawn_health_monitor(db: Database) {
    let _ = thread::Builder::new()
        .name("disk-relocator-health-monitor".to_string())
        .spawn(move || {
            let (event_tx, event_rx) = mpsc::channel();
            let watcher = notify::recommended_watcher(move |result| {
                let _ = event_tx.send(result);
            });

            let mut watcher = match watcher {
                Ok(watcher) => Some(watcher),
                Err(err) => {
                    eprintln!("[health-monitor] mount watcher init failed: {err}");
                    None
                }
            };

            if let Some(w) = watcher.as_mut() {
                if let Err(err) =
                    w.watch(Path::new(VOLUMES_MOUNT_ROOT), RecursiveMode::NonRecursive)
                {
                    eprintln!("[health-monitor] watch /Volumes failed: {err}");
                }
            }

            run_background_cycle(&db, "startup");
            let interval = Duration::from_secs(HEALTH_POLL_INTERVAL_SECS);

            loop {
                match event_rx.recv_timeout(interval) {
                    Ok(Ok(_event)) => run_background_cycle(&db, "mount_event"),
                    Ok(Err(err)) => eprintln!("[health-monitor] watcher event error: {err}"),
                    Err(RecvTimeoutError::Timeout) => run_background_cycle(&db, "poll"),
                    Err(RecvTimeoutError::Disconnected) => {
                        run_background_cycle(&db, "poll_fallback");
                        thread::sleep(interval);
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewRelocationRecord;
    use tempfile::tempdir;

    #[test]
    fn health_check_marks_missing_symlink_as_broken() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source-missing");
        let target_root = dir.path().join("target-root");
        let target = target_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(&target).expect("create target");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_001".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_001", true).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "broken");
        assert_eq!(statuses[0].checks[0].code, "HEALTH_SYMLINK_MISSING");

        let row = db
            .get_relocation("reloc_health_001")
            .expect("query row")
            .expect("row exists");
        assert_eq!(row.state, "BROKEN");
        assert_eq!(row.health_state, "broken");
    }

    #[test]
    fn health_check_marks_offline_target_root_as_degraded() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let missing_root = dir.path().join("missing-volume");
        let target = missing_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(dir.path().join("link-base")).expect("create base");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &source).expect("create source symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_002".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: missing_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_002", false).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "degraded");
        assert_eq!(statuses[0].checks[0].code, "HEALTH_DISK_OFFLINE");
    }

    #[cfg(unix)]
    #[test]
    fn health_check_marks_symlink_target_mismatch_as_degraded() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target_root = dir.path().join("target-root");
        let expected_target = target_root.join("AppData").join("Telegram Desktop");
        let actual_target = target_root.join("AppData").join("Wrong Target");
        fs::create_dir_all(&expected_target).expect("create expected target");
        fs::create_dir_all(&actual_target).expect("create actual target");
        std::os::unix::fs::symlink(&actual_target, &source).expect("create source symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_003".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: expected_target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_003", true).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "degraded");
        assert_eq!(statuses[0].checks[0].code, "HEALTH_METADATA_DRIFT");
    }

    #[cfg(unix)]
    #[test]
    fn health_check_marks_missing_target_as_broken() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target_root = dir.path().join("target-root");
        let target = target_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(&target_root).expect("create target root");
        std::os::unix::fs::symlink(&target, &source).expect("create source symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_004".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_004", false).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "broken");
        assert_eq!(statuses[0].checks[0].code, "HEALTH_TARGET_MISSING");
    }

    #[test]
    fn health_check_marks_non_symlink_source_as_broken() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source-folder");
        let target_root = dir.path().join("target-root");
        let target = target_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(&source).expect("create source folder");
        fs::create_dir_all(&target).expect("create target folder");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_005".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_005", true).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "broken");
        assert_eq!(statuses[0].checks[0].code, "HEALTH_SYMLINK_MISSING");
    }

    #[cfg(unix)]
    #[test]
    fn health_check_can_skip_operation_log_writes() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target_root = dir.path().join("target-root");
        let target = target_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create source symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_006".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_006", false).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "healthy");

        let logs = db
            .list_operation_logs(Some("reloc_health_006"), None)
            .expect("list operation logs");
        assert!(logs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn health_check_writes_operation_log_when_enabled() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target_root = dir.path().join("target-root");
        let target = target_root.join("AppData").join("Telegram Desktop");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create source symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_health_007".to_string(),
            app_id: "telegram-desktop".to_string(),
            tier: "supported".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_health".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let statuses = run_health_check(&db, "tr_health_test_007", true).expect("run health");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].checks[0].code, "HEALTH_RW_PROBE_OK");

        let logs = db
            .list_operation_logs(Some("reloc_health_007"), None)
            .expect("list operation logs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].stage, "health");
        assert_eq!(logs[0].step, "evaluate_relocation");
        assert_eq!(logs[0].status, "succeeded");
    }
}
