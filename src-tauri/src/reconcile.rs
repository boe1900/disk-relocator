use crate::db::{Database, NewHealthSnapshot, NewOperationLogEntry, RelocationRecord};
use crate::migration::cleanup_temp_path;
use crate::models::{ReconcileIssue, ReconcileResult};
use chrono::Utc;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const RECONCILE_POLL_INTERVAL_SECS: u64 = 300;

#[derive(Debug, Clone)]
enum SafeFixAction {
    CleanupTempPath,
    MarkHealthy,
    MarkRolledBack,
    RecreateSourceAndMarkRolledBack,
}

impl SafeFixAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::CleanupTempPath => "cleanup_temp_path",
            Self::MarkHealthy => "mark_state_healthy",
            Self::MarkRolledBack => "mark_state_rolled_back",
            Self::RecreateSourceAndMarkRolledBack => "recreate_source_and_mark_state_rolled_back",
        }
    }
}

#[derive(Debug)]
struct InternalIssue {
    issue: ReconcileIssue,
    safe_fix_action: Option<SafeFixAction>,
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

fn new_monitor_trace_id() -> String {
    format!("tr_reconcile_auto_{}", Uuid::new_v4().simple())
}

fn temp_path_for(record: &RelocationRecord) -> PathBuf {
    PathBuf::from(format!(
        "{}.tmp.{}",
        record.target_path, record.relocation_id
    ))
}

fn mount_root_from_target_root(target_root: &str) -> Option<PathBuf> {
    let trimmed = target_root.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "/Volumes" || trimmed == "/Volumes/" {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/Volumes/") {
        let mount_name = rest
            .split('/')
            .find(|segment| !segment.is_empty())
            .map(str::to_string)?;
        return Some(PathBuf::from(format!("/Volumes/{mount_name}")));
    }
    None
}

fn target_mount_online(record: &RelocationRecord) -> bool {
    mount_root_from_target_root(&record.target_root)
        .map(|mount_root| mount_root.exists())
        .unwrap_or(true)
}

fn is_active_state(state: &str) -> bool {
    matches!(state, "HEALTHY" | "DEGRADED" | "BROKEN")
}

fn new_issue(
    record: &RelocationRecord,
    code: &str,
    severity: &str,
    message: &str,
    suggestion: &str,
    safe_fix_action: Option<SafeFixAction>,
    details: serde_json::Value,
) -> InternalIssue {
    InternalIssue {
        issue: ReconcileIssue {
            relocation_id: record.relocation_id.clone(),
            app_id: record.app_id.clone(),
            code: code.to_string(),
            severity: severity.to_string(),
            message: message.to_string(),
            suggestion: suggestion.to_string(),
            safe_fix_action: safe_fix_action.as_ref().map(|v| v.as_str().to_string()),
            safe_fix_applied: false,
            details,
        },
        safe_fix_action,
    }
}

fn resolve_link_target(source_path: &Path, raw_target: &Path) -> PathBuf {
    if raw_target.is_absolute() {
        raw_target.to_path_buf()
    } else {
        source_path
            .parent()
            .map(|parent| parent.join(raw_target))
            .unwrap_or_else(|| raw_target.to_path_buf())
    }
}

fn evaluate_record(record: &RelocationRecord) -> Vec<InternalIssue> {
    let mut issues = Vec::new();
    let source_path = Path::new(&record.source_path);
    let target_path = Path::new(&record.target_path);
    let backup_path = record.backup_path.as_ref().map(PathBuf::from);
    let temp_path = temp_path_for(record);
    let backup_exists = backup_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(false);
    let target_exists = target_path.exists();
    let temp_exists = temp_path.exists();

    if temp_exists {
        issues.push(new_issue(
            record,
            "RECON_TEMP_PATH_RESIDUE",
            "warning",
            "temporary migration path residue detected.",
            "run safe-fix to cleanup temporary path.",
            Some(SafeFixAction::CleanupTempPath),
            json!({ "temp_path": temp_path }),
        ));
    }

    let source_metadata = match fs::symlink_metadata(source_path) {
        Ok(meta) => meta,
        Err(err) => {
            if !target_exists && !temp_exists && !backup_exists && target_mount_online(record) {
                issues.push(new_issue(
                    record,
                    "RECON_SOURCE_MISSING_RECOVERABLE",
                    "warning",
                    "source path missing while target and backup paths are absent.",
                    "run safe-fix to recreate an empty source path and resync metadata to ROLLED_BACK.",
                    Some(SafeFixAction::RecreateSourceAndMarkRolledBack),
                    json!({
                        "source_path": record.source_path,
                        "target_path": record.target_path,
                        "backup_path": record.backup_path,
                        "target_root": record.target_root,
                        "error": err.to_string()
                    }),
                ));
            } else {
                issues.push(new_issue(
                    record,
                    "RECON_SOURCE_MISSING",
                    "critical",
                    "source path missing during reconciliation.",
                    "run rollback to restore source path, then re-check.",
                    None,
                    json!({ "source_path": record.source_path, "error": err.to_string() }),
                ));
            }
            return issues;
        }
    };

    if source_metadata.file_type().is_symlink() {
        let linked_target = match fs::read_link(source_path) {
            Ok(path) => path,
            Err(err) => {
                issues.push(new_issue(
                    record,
                    "RECON_SYMLINK_READ_FAILED",
                    "critical",
                    "failed to resolve source symlink target.",
                    "run rollback to restore source path, then re-check.",
                    None,
                    json!({ "source_path": record.source_path, "error": err.to_string() }),
                ));
                return issues;
            }
        };

        let linked_target_abs = resolve_link_target(source_path, &linked_target);
        if linked_target_abs != target_path {
            issues.push(new_issue(
                record,
                "RECON_SOURCE_TARGET_MISMATCH",
                "warning",
                "source symlink target mismatches metadata target path.",
                "run rollback and optionally migrate again to recover consistency.",
                None,
                json!({
                    "source_path": record.source_path,
                    "linked_target": linked_target_abs,
                    "expected_target": record.target_path
                }),
            ));
            return issues;
        }

        if !target_path.exists() {
            issues.push(new_issue(
                record,
                "RECON_TARGET_MISSING",
                "critical",
                "source symlink exists but target path is missing.",
                "reconnect target disk or run rollback to recover source path.",
                None,
                json!({
                    "source_path": record.source_path,
                    "target_path": record.target_path
                }),
            ));
        } else if !is_active_state(&record.state) {
            issues.push(new_issue(
                record,
                "RECON_STATE_STALE_ACTIVE",
                "warning",
                "metadata state is stale while filesystem indicates active relocation.",
                "run safe-fix to resync metadata state to HEALTHY.",
                Some(SafeFixAction::MarkHealthy),
                json!({
                    "state": record.state,
                    "source_path": record.source_path,
                    "target_path": record.target_path
                }),
            ));
        }

        if let Some(backup) = backup_path {
            if backup.exists() {
                issues.push(new_issue(
                    record,
                    "RECON_BACKUP_PATH_RESIDUE",
                    "warning",
                    "backup path residue detected after relocation.",
                    "validate backup content then cleanup manually if not needed.",
                    None,
                    json!({ "backup_path": backup }),
                ));
            }
        }
    } else if is_active_state(&record.state) {
        if !target_exists && !temp_exists && !backup_exists {
            issues.push(new_issue(
                record,
                "RECON_STATE_STALE_ROLLED_BACK",
                "warning",
                "metadata indicates active relocation but filesystem looks rolled back.",
                "run safe-fix to resync metadata to ROLLED_BACK.",
                Some(SafeFixAction::MarkRolledBack),
                json!({
                    "state": record.state,
                    "source_path": record.source_path,
                    "target_path": record.target_path
                }),
            ));
        } else {
            issues.push(new_issue(
                record,
                "RECON_EXPECTED_SYMLINK_MISSING",
                "critical",
                "active relocation state expects source symlink, but source is not symlink.",
                "run rollback to recover source path and clear inconsistent state.",
                None,
                json!({
                    "state": record.state,
                    "source_path": record.source_path,
                    "target_path": record.target_path
                }),
            ));
        }
    } else if record.state == "ROLLBACK_FAILED" || record.state == "FAILED_NEEDS_ROLLBACK" {
        if !target_exists && !temp_exists && !backup_exists {
            issues.push(new_issue(
                record,
                "RECON_STATE_STALE_ROLLED_BACK",
                "warning",
                "rollback-failed metadata is stale while filesystem indicates rolled-back state.",
                "run safe-fix to resync metadata to ROLLED_BACK.",
                Some(SafeFixAction::MarkRolledBack),
                json!({
                    "state": record.state,
                    "source_path": record.source_path,
                    "target_path": record.target_path
                }),
            ));
        }
    } else if record.state == "ROLLED_BACK" && target_path.exists() {
        issues.push(new_issue(
            record,
            "RECON_TARGET_RESIDUE_AFTER_ROLLBACK",
            "warning",
            "target path residue detected after rolled-back state.",
            "confirm target data is unnecessary, then clean up manually.",
            None,
            json!({ "target_path": record.target_path }),
        ));
    }

    issues
}

#[allow(clippy::too_many_arguments)]
fn insert_reconcile_log(
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
        stage: "health".to_string(),
        step: step.to_string(),
        status: status.to_string(),
        error_code: error_code.map(str::to_string),
        duration_ms: Some(0),
        message: Some(message.to_string()),
        details_json: details.to_string(),
        created_at: now_iso(),
    })
    .map_err(|err| format!("insert reconcile operation log failed: {err}"))
}

fn apply_safe_fix(
    db: &Database,
    record: &RelocationRecord,
    action: &SafeFixAction,
    trace_id: &str,
) -> Result<(), String> {
    match action {
        SafeFixAction::CleanupTempPath => {
            cleanup_temp_path(&temp_path_for(record))
                .map_err(|err| format!("cleanup temp safe-fix failed: {}", err.message))?;
            Ok(())
        }
        SafeFixAction::MarkHealthy => {
            let observed_at = now_iso();
            db.update_relocation_health(
                &record.relocation_id,
                "HEALTHY",
                "healthy",
                trace_id,
                None,
                &observed_at,
            )
            .map_err(|err| format!("update state healthy safe-fix failed: {err}"))?;
            db.insert_health_snapshot(&NewHealthSnapshot {
                snapshot_id: new_snapshot_id(),
                relocation_id: record.relocation_id.clone(),
                state: "healthy".to_string(),
                check_code: "HEALTH_RECONCILE_RESYNC_ACTIVE".to_string(),
                details_json: json!({ "message": "reconcile safe-fix resynced state to HEALTHY" })
                    .to_string(),
                observed_at,
            })
            .map_err(|err| format!("insert health snapshot safe-fix failed: {err}"))?;
            Ok(())
        }
        SafeFixAction::MarkRolledBack => {
            let observed_at = now_iso();
            db.update_relocation_state(
                &record.relocation_id,
                "ROLLED_BACK",
                "healthy",
                trace_id,
                None,
                &observed_at,
                Some(&observed_at),
            )
            .map_err(|err| format!("update state rolled-back safe-fix failed: {err}"))?;
            db.insert_health_snapshot(&NewHealthSnapshot {
                snapshot_id: new_snapshot_id(),
                relocation_id: record.relocation_id.clone(),
                state: "healthy".to_string(),
                check_code: "HEALTH_RECONCILE_RESYNC_ROLLED_BACK".to_string(),
                details_json: json!({
                    "message": "reconcile safe-fix resynced state to ROLLED_BACK"
                })
                .to_string(),
                observed_at,
            })
            .map_err(|err| format!("insert health snapshot safe-fix failed: {err}"))?;
            Ok(())
        }
        SafeFixAction::RecreateSourceAndMarkRolledBack => {
            let source_path = Path::new(&record.source_path);
            match fs::symlink_metadata(source_path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        return Err(
                            "source path is symlink while recreate-source safe-fix expects directory."
                                .to_string(),
                        );
                    }
                    if !metadata.is_dir() {
                        return Err(
                            "source path exists but is not a directory for recreate-source safe-fix."
                                .to_string(),
                        );
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(source_path).map_err(|create_err| {
                        format!("recreate source path safe-fix failed: {create_err}")
                    })?;
                }
                Err(err) => {
                    return Err(format!("inspect source path safe-fix failed: {err}"));
                }
            }

            let observed_at = now_iso();
            db.update_relocation_state(
                &record.relocation_id,
                "ROLLED_BACK",
                "healthy",
                trace_id,
                None,
                &observed_at,
                Some(&observed_at),
            )
            .map_err(|err| format!("update state rolled-back safe-fix failed: {err}"))?;
            db.insert_health_snapshot(&NewHealthSnapshot {
                snapshot_id: new_snapshot_id(),
                relocation_id: record.relocation_id.clone(),
                state: "healthy".to_string(),
                check_code: "HEALTH_RECONCILE_RECREATE_SOURCE_ROLLED_BACK".to_string(),
                details_json: json!({
                    "message": "reconcile safe-fix recreated empty source path and resynced state to ROLLED_BACK",
                    "source_path": record.source_path,
                    "data_recovered": false
                })
                .to_string(),
                observed_at,
            })
            .map_err(|err| format!("insert health snapshot safe-fix failed: {err}"))?;
            Ok(())
        }
    }
}

pub fn run_reconcile(
    db: &Database,
    trace_id: &str,
    apply_safe_fixes: bool,
    limit: usize,
    write_operation_logs: bool,
) -> Result<ReconcileResult, String> {
    let records = db
        .list_relocations()
        .map_err(|err| format!("list relocations for reconcile failed: {err}"))?;
    let mut selected = Vec::new();
    let mut seen_pairs = HashSet::new();
    for record in records {
        let key = (record.app_id.clone(), record.source_path.clone());
        if seen_pairs.insert(key) {
            selected.push(record);
            if selected.len() >= limit {
                break;
            }
        }
    }

    let mut issues = Vec::new();
    let mut fixed_count = 0usize;
    let mut safe_fixable_count = 0usize;

    for record in selected.iter() {
        let found = evaluate_record(record);
        for mut internal in found {
            if internal.safe_fix_action.is_some() {
                safe_fixable_count += 1;
            }

            if write_operation_logs {
                insert_reconcile_log(
                    db,
                    &record.relocation_id,
                    trace_id,
                    "reconcile_detect",
                    "failed",
                    Some(&internal.issue.code),
                    &internal.issue.message,
                    json!({
                        "severity": internal.issue.severity,
                        "suggestion": internal.issue.suggestion,
                        "details": internal.issue.details
                    }),
                )?;
            }

            if apply_safe_fixes {
                if let Some(action) = internal.safe_fix_action.clone() {
                    let fix_result = apply_safe_fix(db, record, &action, trace_id);
                    if fix_result.is_ok() {
                        internal.issue.safe_fix_applied = true;
                        fixed_count += 1;
                    }

                    if write_operation_logs {
                        match fix_result {
                            Ok(()) => {
                                insert_reconcile_log(
                                    db,
                                    &record.relocation_id,
                                    trace_id,
                                    "reconcile_safe_fix",
                                    "succeeded",
                                    None,
                                    "reconcile safe-fix applied.",
                                    json!({ "action": action.as_str(), "code": internal.issue.code }),
                                )?;
                            }
                            Err(err) => {
                                insert_reconcile_log(
                                    db,
                                    &record.relocation_id,
                                    trace_id,
                                    "reconcile_safe_fix",
                                    "failed",
                                    Some("HEALTH_METADATA_DRIFT"),
                                    "reconcile safe-fix failed.",
                                    json!({
                                        "action": action.as_str(),
                                        "code": internal.issue.code,
                                        "error": err
                                    }),
                                )?;
                            }
                        }
                    }
                }
            }

            issues.push(internal.issue);
        }
    }

    Ok(ReconcileResult {
        trace_id: trace_id.to_string(),
        observed_at: now_iso(),
        scanned: selected.len(),
        drift_count: issues.len(),
        safe_fixable_count,
        fixed_count,
        issues,
    })
}

pub fn spawn_reconcile_monitor(db: Database) {
    let _ = thread::Builder::new()
        .name("disk-relocator-reconcile-monitor".to_string())
        .spawn(move || loop {
            let trace_id = new_monitor_trace_id();
            if let Err(err) = run_reconcile(&db, &trace_id, true, 500, false) {
                eprintln!("[reconcile-monitor] run failed: {err}");
            }
            thread::sleep(Duration::from_secs(RECONCILE_POLL_INTERVAL_SECS));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewRelocationRecord;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn reconcile_detects_temp_residue() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_001".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let temp = dir.path().join("target.tmp.reloc_reconcile_001");
        fs::create_dir_all(&temp).expect("create temp residue");

        let result =
            run_reconcile(&db, "tr_reconcile_test_1", false, 50, false).expect("reconcile");
        assert_eq!(result.scanned, 1);
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.issues[0].code, "RECON_TEMP_PATH_RESIDUE");
        assert_eq!(
            result.issues[0].safe_fix_action.as_deref(),
            Some("cleanup_temp_path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_safe_fix_cleans_temp_residue() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_003".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let temp = dir.path().join("target.tmp.reloc_reconcile_003");
        fs::create_dir_all(&temp).expect("create temp residue");
        assert!(temp.exists());

        let result = run_reconcile(&db, "tr_reconcile_test_3", true, 50, true).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 1);
        assert_eq!(result.issues[0].code, "RECON_TEMP_PATH_RESIDUE");
        assert!(result.issues[0].safe_fix_applied);
        assert!(!temp.exists());
    }

    #[test]
    fn reconcile_safe_fix_marks_stale_state_as_rolled_back() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        fs::create_dir_all(&source).expect("create source");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_002".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "bootstrap".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: dir
                .path()
                .join("target-missing")
                .to_string_lossy()
                .to_string(),
            backup_path: None,
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let result = run_reconcile(&db, "tr_reconcile_test_2", true, 50, true).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 1);
        assert_eq!(result.issues[0].code, "RECON_STATE_STALE_ROLLED_BACK");
        assert!(result.issues[0].safe_fix_applied);

        let row = db
            .get_relocation("reloc_reconcile_002")
            .expect("query relocation")
            .expect("row exists");
        assert_eq!(row.state, "ROLLED_BACK");
        assert_eq!(row.health_state, "healthy");
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_safe_fix_marks_stale_active_state_as_healthy() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_004".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "PRECHECKING".to_string(),
            health_state: "degraded".to_string(),
            last_error_code: Some("PREVIOUS_ERROR".to_string()),
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: None,
        })
        .expect("insert relocation");

        let result = run_reconcile(&db, "tr_reconcile_test_4", true, 50, true).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 1);
        assert_eq!(result.issues[0].code, "RECON_STATE_STALE_ACTIVE");
        assert!(result.issues[0].safe_fix_applied);

        let row = db
            .get_relocation("reloc_reconcile_004")
            .expect("query relocation")
            .expect("row exists");
        assert_eq!(row.state, "HEALTHY");
        assert_eq!(row.health_state, "healthy");
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_ignores_historical_records_and_only_checks_latest_source_entry() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_hist_old".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "PRECHECK_FAILED".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: Some("PRECHECK_TARGET_PATH_EXISTS".to_string()),
            trace_id: "tr_seed_old".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:01:00Z".to_string(),
            completed_at: Some("2026-03-05T10:01:00Z".to_string()),
        })
        .expect("insert historical relocation");

        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_hist_latest".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed_latest".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: "2026-03-05T10:02:00Z".to_string(),
            updated_at: "2026-03-05T10:03:00Z".to_string(),
            completed_at: Some("2026-03-05T10:03:00Z".to_string()),
        })
        .expect("insert latest relocation");

        let result = run_reconcile(&db, "tr_reconcile_hist", true, 50, false).expect("reconcile");
        assert_eq!(result.scanned, 1);
        assert_eq!(result.drift_count, 0);
        assert_eq!(result.fixed_count, 0);
        assert!(result.issues.is_empty());

        let old_row = db
            .get_relocation("reloc_reconcile_hist_old")
            .expect("query old row")
            .expect("old row exists");
        assert_eq!(old_row.state, "PRECHECK_FAILED");
    }

    #[test]
    fn reconcile_detects_expected_symlink_missing_without_safe_fix() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(&target).expect("create target dir");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_005".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let result = run_reconcile(&db, "tr_reconcile_test_5", true, 50, false).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 0);
        assert_eq!(result.issues[0].code, "RECON_EXPECTED_SYMLINK_MISSING");
        assert!(!result.issues[0].safe_fix_applied);
        assert!(result.issues[0].safe_fix_action.is_none());
    }

    #[test]
    fn reconcile_marks_missing_source_as_critical_issue() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("missing-source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_006".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let result = run_reconcile(&db, "tr_reconcile_test_6", true, 50, false).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 0);
        assert_eq!(result.issues[0].code, "RECON_SOURCE_MISSING");
        assert_eq!(result.issues[0].severity, "critical");
        assert!(!result.issues[0].safe_fix_applied);
        assert!(result.issues[0].safe_fix_action.is_none());
    }

    #[test]
    fn reconcile_safe_fix_recreates_missing_source_when_recovery_sources_absent() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("missing-source-recreate");
        let target = dir.path().join("missing-target-recreate");
        assert!(!source.exists());
        assert!(!target.exists());

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_recreate_source".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "ROLLBACK_FAILED".to_string(),
            health_state: "broken".to_string(),
            last_error_code: Some("ROLLBACK_RESTORE_BACKUP_FAILED".to_string()),
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let result =
            run_reconcile(&db, "tr_reconcile_recreate_source", true, 50, false).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.safe_fixable_count, 1);
        assert_eq!(result.fixed_count, 1);
        assert_eq!(result.issues[0].code, "RECON_SOURCE_MISSING_RECOVERABLE");
        assert_eq!(
            result.issues[0].safe_fix_action.as_deref(),
            Some("recreate_source_and_mark_state_rolled_back")
        );
        assert!(result.issues[0].safe_fix_applied);
        assert!(source.is_dir());

        let row = db
            .get_relocation("reloc_reconcile_recreate_source")
            .expect("query relocation")
            .expect("row exists");
        assert_eq!(row.state, "ROLLED_BACK");
        assert_eq!(row.health_state, "healthy");
    }

    #[test]
    fn reconcile_missing_source_keeps_critical_when_target_mount_offline() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("missing-source-offline");
        let mount_name = format!("reconcile_offline_{}", Uuid::new_v4().simple());
        let target_root = format!("/Volumes/{mount_name}");
        let target = format!("{target_root}/AppData/Telegram/media");
        assert!(!Path::new(&target_root).exists());

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_offline_mount".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: target_root.clone(),
            target_path: target,
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "ROLLBACK_FAILED".to_string(),
            health_state: "broken".to_string(),
            last_error_code: Some("ROLLBACK_RESTORE_BACKUP_FAILED".to_string()),
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let result =
            run_reconcile(&db, "tr_reconcile_offline_mount", true, 50, false).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.safe_fixable_count, 0);
        assert_eq!(result.fixed_count, 0);
        assert_eq!(result.issues[0].code, "RECON_SOURCE_MISSING");
        assert_eq!(result.issues[0].severity, "critical");
        assert!(result.issues[0].safe_fix_action.is_none());
        assert!(!result.issues[0].safe_fix_applied);
        assert!(!source.exists());
    }

    #[test]
    fn reconcile_respects_scan_limit() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        for index in 0..2 {
            let source = dir.path().join(format!("missing-source-{index}"));
            let target = dir.path().join(format!("target-{index}"));
            fs::create_dir_all(&target).expect("create target dir");

            let now = "2026-03-05T10:00:00Z".to_string();
            db.insert_relocation(&NewRelocationRecord {
                relocation_id: format!("reloc_reconcile_limit_{index}"),
                app_id: "telegram-desktop".to_string(),
                mode: "migrate".to_string(),
                source_path: source.to_string_lossy().to_string(),
                target_root: dir.path().to_string_lossy().to_string(),
                target_path: target.to_string_lossy().to_string(),
                backup_path: Some(format!("{}.bak", source.to_string_lossy())),
                state: "HEALTHY".to_string(),
                health_state: "healthy".to_string(),
                last_error_code: None,
                trace_id: "tr_seed".to_string(),
                source_size_bytes: 0,
                target_size_bytes: 0,
                created_at: now.clone(),
                updated_at: format!("2026-03-05T10:00:0{index}Z"),
                completed_at: Some(now),
            })
            .expect("insert relocation");
        }

        let result = run_reconcile(&db, "tr_reconcile_limit", false, 1, false).expect("reconcile");
        assert_eq!(result.scanned, 1);
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.issues[0].code, "RECON_SOURCE_MISSING");
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_writes_detect_and_safe_fix_logs_when_enabled() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &source).expect("create symlink");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_log_001".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: Some(now),
        })
        .expect("insert relocation");

        let temp = dir.path().join("target.tmp.reloc_reconcile_log_001");
        fs::create_dir_all(&temp).expect("create temp residue");

        let result = run_reconcile(&db, "tr_reconcile_log", true, 50, true).expect("reconcile");
        assert_eq!(result.drift_count, 1);
        assert_eq!(result.fixed_count, 1);

        let logs = db
            .list_operation_logs(Some("reloc_reconcile_log_001"), Some("tr_reconcile_log"))
            .expect("list reconcile logs");
        assert!(logs
            .iter()
            .any(|log| log.step == "reconcile_detect" && log.status == "failed"));
        assert!(logs
            .iter()
            .any(|log| { log.step == "reconcile_safe_fix" && log.status == "succeeded" }));
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_reports_safe_fixable_count_for_mixed_issues() {
        let dir = tempdir().expect("tempdir");
        let db = Database::init(dir.path().join("db")).expect("init db");

        let source_a = dir.path().join("source-a");
        let target_a = dir.path().join("target-a");
        fs::create_dir_all(&target_a).expect("create target a");
        std::os::unix::fs::symlink(&target_a, &source_a).expect("create symlink a");

        let now = "2026-03-05T10:00:00Z".to_string();
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_mixed_a".to_string(),
            app_id: "telegram-desktop".to_string(),
            mode: "migrate".to_string(),
            source_path: source_a.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target_a.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source_a.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: "2026-03-05T10:00:01Z".to_string(),
            completed_at: Some(now.clone()),
        })
        .expect("insert relocation a");
        let temp = dir.path().join("target-a.tmp.reloc_reconcile_mixed_a");
        fs::create_dir_all(&temp).expect("create temp residue");

        let source_b = dir.path().join("missing-source-b");
        let target_b = dir.path().join("target-b");
        fs::create_dir_all(&target_b).expect("create target b");
        db.insert_relocation(&NewRelocationRecord {
            relocation_id: "reloc_reconcile_mixed_b".to_string(),
            app_id: "wechat-non-mas".to_string(),
            mode: "migrate".to_string(),
            source_path: source_b.to_string_lossy().to_string(),
            target_root: dir.path().to_string_lossy().to_string(),
            target_path: target_b.to_string_lossy().to_string(),
            backup_path: Some(format!("{}.bak", source_b.to_string_lossy())),
            state: "HEALTHY".to_string(),
            health_state: "healthy".to_string(),
            last_error_code: None,
            trace_id: "tr_seed".to_string(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: "2026-03-05T10:00:02Z".to_string(),
            completed_at: Some(now),
        })
        .expect("insert relocation b");

        let result = run_reconcile(&db, "tr_reconcile_mixed", false, 50, false).expect("reconcile");
        assert_eq!(result.scanned, 2);
        assert_eq!(result.drift_count, 2);
        assert_eq!(result.safe_fixable_count, 1);
        assert_eq!(result.fixed_count, 0);
    }
}
