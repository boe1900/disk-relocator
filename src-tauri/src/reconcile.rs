use crate::db::{Database, NewHealthSnapshot, NewOperationLogEntry, RelocationRecord};
use crate::migration::cleanup_temp_path;
use crate::models::{ReconcileIssue, ReconcileResult};
use chrono::Utc;
use serde_json::json;
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
}

impl SafeFixAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::CleanupTempPath => "cleanup_temp_path",
            Self::MarkHealthy => "mark_state_healthy",
            Self::MarkRolledBack => "mark_state_rolled_back",
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

    if temp_path.exists() {
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
            issues.push(new_issue(
                record,
                "RECON_SOURCE_MISSING",
                "critical",
                "source path missing during reconciliation.",
                "run rollback to restore source path, then re-check.",
                None,
                json!({ "source_path": record.source_path, "error": err.to_string() }),
            ));
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
    } else {
        if is_active_state(&record.state) {
            let backup_exists = backup_path
                .as_ref()
                .map(|path| path.exists())
                .unwrap_or(false);
            if !target_path.exists() && !temp_path.exists() && !backup_exists {
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
    }

    issues
}

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
    let selected: Vec<RelocationRecord> = records.into_iter().take(limit).collect();

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
            if let Err(err) = run_reconcile(&db, &trace_id, false, 500, false) {
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
            tier: "supported".to_string(),
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
            tier: "supported".to_string(),
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
}
