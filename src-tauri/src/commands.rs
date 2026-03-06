use crate::db::{
    Database, HealthEventRecord, NewHealthSnapshot, NewOperationLogEntry, NewRelocationRecord,
    OperationLogRecord,
};
use crate::models::{
    AppScanPath, AppScanResult, CommandError, DiskStatus, ExportLogsRequest, ExportLogsResult,
    HealthEvent, HealthEventsRequest, HealthStatus, MigrateRequest, OperationLogItem,
    ReconcileRequest, ReconcileResult, RelocationResult, RelocationSummary, RollbackRequest,
};
use crate::profiles::{self, AppProfile};
use crate::{
    bootstrap::{execute_bootstrap_switch, rollback_bootstrap_switch, BootstrapSwitchError},
    health,
    migration::{
        cleanup_temp_path, copy_path_to_path, copy_source_to_temp, remove_path_if_exists,
        rollback_migration_paths, switch_to_symlink, verify_source_and_temp, MigrationError,
    },
    reconcile, AppState,
};
use chrono::Utc;
use fs2::{available_space, total_space};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use sysinfo::{ProcessesToUpdate, System};
use tauri::State;
use uuid::Uuid;

const SAFETY_MARGIN_MIN_BYTES: u64 = 512 * 1024 * 1024;

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn new_trace_id() -> String {
    format!("tr_{}", Uuid::new_v4().simple())
}

fn new_relocation_id() -> String {
    format!("reloc_{}", Uuid::new_v4().simple())
}

fn new_log_id() -> String {
    format!("log_{}", Uuid::new_v4().simple())
}

fn new_snapshot_id() -> String {
    format!("snap_{}", Uuid::new_v4().simple())
}

fn expand_tilde(path: &str) -> String {
    if let Some(suffix) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{suffix}");
        }
    }
    path.to_string()
}

fn target_path(profile: &AppProfile, target_root: &str) -> String {
    if profile.target_path_template.is_empty() {
        return format!("{target_root}/AppData/{}", profile.display_name);
    }
    profile
        .target_path_template
        .replace("{target_root}", target_root)
}

fn db_error(trace_id: &str, action: &str, err: impl ToString) -> CommandError {
    CommandError::new(
        "DB_OPERATION_FAILED",
        format!("{action} failed"),
        trace_id,
        true,
        json!({ "action": action, "error": err.to_string() }),
    )
}

fn profile_error(trace_id: &str, err: &str) -> CommandError {
    CommandError::new(
        "PROFILE_PARSE_FAILED",
        "Failed to load app profiles.",
        trace_id,
        false,
        json!({ "error": err }),
    )
}

fn relocation_to_result(row: crate::db::RelocationRecord) -> RelocationResult {
    RelocationResult {
        relocation_id: row.relocation_id,
        app_id: row.app_id,
        state: row.state,
        health_state: row.health_state,
        source_path: row.source_path,
        target_path: row.target_path,
        backup_path: row.backup_path,
        trace_id: row.trace_id,
        started_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn operation_log_to_item(row: OperationLogRecord) -> OperationLogItem {
    let details =
        serde_json::from_str::<serde_json::Value>(&row.details_json).unwrap_or_else(|_| {
            json!({
                "_parse_error": "invalid details_json payload",
                "_raw": row.details_json
            })
        });

    OperationLogItem {
        log_id: row.log_id,
        relocation_id: row.relocation_id,
        trace_id: row.trace_id,
        stage: row.stage,
        step: row.step,
        status: row.status,
        error_code: row.error_code,
        duration_ms: row.duration_ms,
        message: row.message,
        details,
        created_at: row.created_at,
    }
}

fn default_export_output_path(db: &Database, trace_id: &str) -> PathBuf {
    let base_dir = db
        .path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    base_dir
        .join("exports")
        .join(format!("{trace_id}.operation-logs.json"))
}

fn health_event_to_model(row: HealthEventRecord) -> HealthEvent {
    let message = serde_json::from_str::<serde_json::Value>(&row.details_json)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|msg| msg.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "health event captured".to_string());

    HealthEvent {
        snapshot_id: row.snapshot_id,
        relocation_id: row.relocation_id,
        app_id: row.app_id,
        state: row.state,
        check_code: row.check_code,
        message,
        observed_at: row.observed_at,
    }
}

fn insert_operation_log(
    db: &Database,
    trace_id: &str,
    relocation_id: &str,
    stage: &str,
    step: &str,
    status: &str,
    error_code: Option<&str>,
    duration_ms: Option<i64>,
    message: &str,
    details: serde_json::Value,
) -> Result<(), CommandError> {
    db.insert_operation_log(&NewOperationLogEntry {
        log_id: new_log_id(),
        relocation_id: relocation_id.to_string(),
        trace_id: trace_id.to_string(),
        stage: stage.to_string(),
        step: step.to_string(),
        status: status.to_string(),
        error_code: error_code.map(str::to_string),
        duration_ms,
        message: Some(message.to_string()),
        details_json: details.to_string(),
        created_at: now_iso(),
    })
    .map_err(|err| db_error(trace_id, "insert operation log", err))
}

fn run_precheck_step<F>(
    state: &AppState,
    relocation_id: &str,
    trace_id: &str,
    step: &str,
    action: F,
) -> Result<serde_json::Value, CommandError>
where
    F: FnOnce() -> Result<serde_json::Value, CommandError>,
{
    insert_operation_log(
        &state.db,
        trace_id,
        relocation_id,
        "precheck",
        step,
        "started",
        None,
        None,
        "precheck started",
        json!({}),
    )?;

    let timer = Instant::now();
    match action() {
        Ok(details) => {
            insert_operation_log(
                &state.db,
                trace_id,
                relocation_id,
                "precheck",
                step,
                "succeeded",
                None,
                Some(timer.elapsed().as_millis() as i64),
                "precheck succeeded",
                details.clone(),
            )?;
            Ok(details)
        }
        Err(err) => {
            insert_operation_log(
                &state.db,
                trace_id,
                relocation_id,
                "precheck",
                step,
                "failed",
                Some(&err.code),
                Some(timer.elapsed().as_millis() as i64),
                &err.message,
                err.details.clone(),
            )?;
            Err(err)
        }
    }
}

fn run_precheck_step_guarded<F>(
    state: &AppState,
    relocation_id: &str,
    trace_id: &str,
    step: &str,
    action: F,
) -> Result<serde_json::Value, CommandError>
where
    F: FnOnce() -> Result<serde_json::Value, CommandError>,
{
    match run_precheck_step(state, relocation_id, trace_id, step, action) {
        Ok(value) => Ok(value),
        Err(err) => {
            state
                .db
                .update_relocation_state(
                    relocation_id,
                    "PRECHECK_FAILED",
                    "unknown",
                    trace_id,
                    Some(&err.code),
                    &now_iso(),
                    None,
                )
                .map_err(|update_err| db_error(trace_id, "update relocation state", update_err))?;
            Err(err)
        }
    }
}

fn precheck_error(
    code: &str,
    message: &str,
    trace_id: &str,
    retryable: bool,
    details: serde_json::Value,
) -> CommandError {
    CommandError::new(code, message, trace_id, retryable, details)
}

fn bootstrap_error(trace_id: &str, err: BootstrapSwitchError) -> CommandError {
    CommandError::new(err.code, err.message, trace_id, err.retryable, err.details)
}

fn migration_error(trace_id: &str, err: MigrationError) -> CommandError {
    CommandError::new(err.code, err.message, trace_id, err.retryable, err.details)
}

fn detect_running_processes(profile: &AppProfile) -> Vec<String> {
    if profile.process_names.is_empty() || !profile.precheck_rules.require_process_stopped {
        return Vec::new();
    }

    let expected: Vec<String> = profile
        .process_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();

    let mut system = System::new_all();
    let _ = system.refresh_processes(ProcessesToUpdate::All, true);

    let mut running = Vec::new();
    for process in system.processes().values() {
        let process_name = process.name().to_string_lossy().to_string();
        let process_name_lc = process_name.to_ascii_lowercase();
        if expected.iter().any(|expected_name| {
            process_name_lc == *expected_name || process_name_lc.contains(expected_name)
        }) {
            running.push(process_name);
        }
    }

    running.sort();
    running.dedup();
    running
}

fn list_installed_app_bundle_names() -> Vec<String> {
    let mut roots = vec![PathBuf::from("/Applications"), PathBuf::from("/System/Applications")];
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }

    let mut names = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let extension = path.extension().and_then(|ext| ext.to_str());
            if extension != Some("app") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(stem.to_ascii_lowercase());
            }
        }
    }

    names.sort();
    names.dedup();
    names
}

fn profile_looks_installed(profile: &AppProfile, installed_bundles: &[String]) -> bool {
    if installed_bundles.is_empty() {
        return false;
    }

    let mut hints: Vec<String> = profile
        .process_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    hints.push(profile.display_name.to_ascii_lowercase());
    hints.push(profile.app_id.to_ascii_lowercase());

    if profile.app_id == "jetbrains-caches" {
        hints.extend(
            [
                "intellij idea",
                "pycharm",
                "webstorm",
                "goland",
                "clion",
                "rubymine",
                "datagrip",
                "android studio",
            ]
            .into_iter()
            .map(str::to_string),
        );
    }

    installed_bundles.iter().any(|bundle| {
        hints.iter().any(|hint| {
            let trimmed = hint.trim();
            !trimmed.is_empty() && (bundle.contains(trimmed) || trimmed.contains(bundle))
        })
    })
}

fn list_mounted_volume_roots() -> Vec<String> {
    let mut roots = Vec::new();
    let Ok(entries) = fs::read_dir("/Volumes") else {
        return roots;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        roots.push(path.to_string_lossy().to_string());
    }

    roots.sort();
    roots.dedup();
    roots
}

fn directory_size(path: &Path) -> std::io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total.saturating_add(directory_size(&entry.path())?);
    }
    Ok(total)
}

fn check_source(
    mode: &str,
    source_path: &str,
    allow_bootstrap_if_missing: bool,
    require_full_disk_access: bool,
    trace_id: &str,
) -> Result<u64, CommandError> {
    let path = Path::new(source_path);
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if mode == "bootstrap" && allow_bootstrap_if_missing {
                return Ok(0);
            }
            return Err(precheck_error(
                "PRECHECK_SOURCE_NOT_FOUND",
                "source path not found for migration mode.",
                trace_id,
                false,
                json!({ "source_path": source_path, "mode": mode }),
            ));
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(precheck_error(
                "PRECHECK_PERMISSION_DENIED",
                "missing permission to read source path.",
                trace_id,
                false,
                json!({ "source_path": source_path, "error": err.to_string() }),
            ));
        }
        Err(err) => {
            return Err(precheck_error(
                "PRECHECK_SOURCE_NOT_FOUND",
                "failed to inspect source path.",
                trace_id,
                false,
                json!({ "source_path": source_path, "error": err.to_string() }),
            ));
        }
    };

    if metadata.file_type().is_symlink() {
        return Err(precheck_error(
            "PRECHECK_SOURCE_IS_SYMLINK",
            "source path is already a symlink.",
            trace_id,
            false,
            json!({ "source_path": source_path }),
        ));
    }

    if require_full_disk_access {
        match fs::read_dir(path) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(precheck_error(
                    "PRECHECK_PERMISSION_DENIED",
                    "Full Disk Access permission appears missing.",
                    trace_id,
                    false,
                    json!({ "source_path": source_path, "error": err.to_string() }),
                ));
            }
            Err(_) => {}
        }
    }

    directory_size(path).map_err(|err| {
        let (code, retryable) = if err.kind() == std::io::ErrorKind::PermissionDenied {
            ("PRECHECK_PERMISSION_DENIED", false)
        } else {
            ("PRECHECK_SOURCE_NOT_FOUND", true)
        };
        precheck_error(
            code,
            "failed to estimate source data size.",
            trace_id,
            retryable,
            json!({ "source_path": source_path, "error": err.to_string() }),
        )
    })
}

fn check_target_online(target_root: &str, trace_id: &str) -> Result<(), CommandError> {
    let target = Path::new(target_root);
    if target.is_dir() {
        return Ok(());
    }

    Err(precheck_error(
        "PRECHECK_DISK_OFFLINE",
        "target disk is offline or not mounted.",
        trace_id,
        true,
        json!({ "target_root": target_root }),
    ))
}

fn check_target_writable(target_root: &str, trace_id: &str) -> Result<(), CommandError> {
    let probe_path =
        Path::new(target_root).join(format!(".disk-relocator-probe-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path)
        .map_err(|err| {
            let code = if err.kind() == std::io::ErrorKind::PermissionDenied {
                "PRECHECK_DISK_READONLY"
            } else {
                "PRECHECK_DISK_OFFLINE"
            };
            precheck_error(
                code,
                "failed to create writable probe on target root.",
                trace_id,
                true,
                json!({ "target_root": target_root, "error": err.to_string() }),
            )
        })?;

    file.write_all(b"ok").map_err(|err| {
        precheck_error(
            "PRECHECK_DISK_READONLY",
            "failed to write probe file on target root.",
            trace_id,
            true,
            json!({ "target_root": target_root, "error": err.to_string() }),
        )
    })?;

    fs::remove_file(&probe_path).map_err(|err| {
        precheck_error(
            "PRECHECK_DISK_READONLY",
            "failed to cleanup writable probe file.",
            trace_id,
            true,
            json!({ "target_root": target_root, "error": err.to_string() }),
        )
    })
}

fn check_available_space(
    target_root: &str,
    source_size_bytes: u64,
    trace_id: &str,
) -> Result<(u64, u64), CommandError> {
    let free_bytes = available_space(Path::new(target_root)).map_err(|err| {
        precheck_error(
            "PRECHECK_DISK_OFFLINE",
            "failed to query available disk space.",
            trace_id,
            true,
            json!({ "target_root": target_root, "error": err.to_string() }),
        )
    })?;

    let safety_margin = std::cmp::max(SAFETY_MARGIN_MIN_BYTES, source_size_bytes / 10);
    let required_bytes = source_size_bytes.saturating_add(safety_margin);

    if free_bytes < required_bytes {
        return Err(precheck_error(
            "PRECHECK_INSUFFICIENT_SPACE",
            "target disk has insufficient free space.",
            trace_id,
            true,
            json!({
              "target_root": target_root,
              "free_bytes": free_bytes,
              "required_bytes": required_bytes,
              "source_size_bytes": source_size_bytes,
              "safety_margin_bytes": safety_margin
            }),
        ));
    }

    Ok((free_bytes, required_bytes))
}

fn postcheck_symlink_target(
    source_path: &str,
    target_path: &str,
    trace_id: &str,
) -> Result<serde_json::Value, CommandError> {
    let source = Path::new(source_path);
    let target = Path::new(target_path);

    let source_metadata = fs::symlink_metadata(source).map_err(|err| {
        CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "failed to inspect source path after bootstrap switch.",
            trace_id,
            false,
            json!({ "source_path": source_path, "error": err.to_string() }),
        )
    })?;
    if !source_metadata.file_type().is_symlink() {
        return Err(CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "source path is not a symlink after bootstrap switch.",
            trace_id,
            false,
            json!({ "source_path": source_path }),
        ));
    }

    let linked_target = fs::read_link(source).map_err(|err| {
        CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "failed to resolve source symlink target.",
            trace_id,
            false,
            json!({ "source_path": source_path, "error": err.to_string() }),
        )
    })?;
    if linked_target != target {
        return Err(CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "source symlink target does not match expected target path.",
            trace_id,
            false,
            json!({
              "source_path": source_path,
              "linked_target": linked_target,
              "expected_target": target_path
            }),
        ));
    }

    let target_metadata = fs::symlink_metadata(target).map_err(|err| {
        CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "failed to inspect target path after bootstrap switch.",
            trace_id,
            false,
            json!({ "target_path": target_path, "error": err.to_string() }),
        )
    })?;
    if !target_metadata.is_dir() {
        return Err(CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "target path is not a directory after bootstrap switch.",
            trace_id,
            false,
            json!({ "target_path": target_path }),
        ));
    }

    let probe_file = target.join(format!(".disk-relocator-postcheck-{}", Uuid::new_v4()));
    let mut probe = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_file)
        .map_err(|err| {
            CommandError::new(
                "MIGRATE_POSTCHECK_FAILED",
                "failed to create writable probe in target path.",
                trace_id,
                false,
                json!({ "target_path": target_path, "error": err.to_string() }),
            )
        })?;
    probe.write_all(b"ok").map_err(|err| {
        CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "failed to write writable probe in target path.",
            trace_id,
            false,
            json!({ "target_path": target_path, "error": err.to_string() }),
        )
    })?;
    fs::remove_file(&probe_file).map_err(|err| {
        CommandError::new(
            "MIGRATE_POSTCHECK_FAILED",
            "failed to clean writable probe in target path.",
            trace_id,
            false,
            json!({ "target_path": target_path, "error": err.to_string() }),
        )
    })?;

    Ok(json!({
      "source_path": source_path,
      "target_path": target_path,
      "symlink_ok": true,
      "target_writable_ok": true
    }))
}

#[tauri::command]
pub fn scan_apps() -> Result<Vec<AppScanResult>, CommandError> {
    let trace_id = new_trace_id();
    let profiles = profiles::list_profiles().map_err(|err| profile_error(&trace_id, &err))?;
    let installed_bundles = list_installed_app_bundle_names();
    let now = now_iso();
    let results = profiles
        .into_iter()
        .filter_map(|profile| {
            let running = !detect_running_processes(&profile).is_empty();
            let installed = profile_looks_installed(&profile, &installed_bundles);
            let mut detected_any_path = false;
            let detected_paths = profile
                .source_paths
                .iter()
                .map(|path| {
                    let expanded = expand_tilde(path);
                    let source = Path::new(&expanded);
                    match fs::symlink_metadata(source) {
                        Ok(metadata) => {
                            detected_any_path = true;
                            let is_symlink = metadata.file_type().is_symlink();
                            let size_bytes = if is_symlink {
                                0
                            } else {
                                directory_size(source).unwrap_or(0)
                            };
                            AppScanPath {
                                path: expanded,
                                exists: true,
                                is_symlink,
                                size_bytes,
                            }
                        }
                        Err(_) => AppScanPath {
                            path: expanded,
                            exists: false,
                            is_symlink: false,
                            size_bytes: 0,
                        },
                    }
                })
                .collect::<Vec<_>>();

            if !detected_any_path && !running && !installed {
                return None;
            }

            Some(AppScanResult {
                app_id: profile.app_id,
                display_name: profile.display_name,
                tier: profile.tier,
                detected_paths,
                running,
                allow_bootstrap_if_source_missing: profile
                    .precheck_rules
                    .allow_bootstrap_if_source_missing,
                last_verified_at: now.clone(),
            })
        })
        .collect::<Vec<_>>();

    Ok(results)
}

#[tauri::command]
pub fn get_disk_status(state: State<'_, AppState>) -> Result<Vec<DiskStatus>, CommandError> {
    let trace_id = new_trace_id();
    let records = state
        .db
        .list_health_monitoring_relocations()
        .map_err(|err| db_error(&trace_id, "list monitor relocations", err))?;

    let mut roots = BTreeSet::new();
    for row in records {
        roots.insert(row.target_root);
    }
    for root in list_mounted_volume_roots() {
        roots.insert(root);
    }

    let mut result = Vec::new();
    for root in roots {
        let root_path = Path::new(&root);
        let is_mounted = root_path.is_dir();

        let is_writable = if is_mounted {
            let probe_path = root_path.join(format!(
                ".disk-relocator-disk-status-probe-{}",
                Uuid::new_v4()
            ));
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&probe_path)
            {
                Ok(mut file) => {
                    let write_ok = file.write_all(b"ok").is_ok();
                    let _ = fs::remove_file(&probe_path);
                    write_ok
                }
                Err(_) => false,
            }
        } else {
            false
        };

        let free_bytes = if is_mounted {
            available_space(root_path).unwrap_or(0)
        } else {
            0
        };
        let total_bytes = if is_mounted {
            total_space(root_path).unwrap_or(0)
        } else {
            0
        };
        let display_name = root_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&root)
            .to_string();

        result.push(DiskStatus {
            mount_point: root,
            display_name,
            is_mounted,
            is_writable,
            free_bytes,
            total_bytes,
        });
    }

    Ok(result)
}

#[tauri::command]
pub fn migrate_app(
    req: MigrateRequest,
    state: State<'_, AppState>,
) -> Result<RelocationResult, CommandError> {
    let trace_id = new_trace_id();

    if req.mode != "bootstrap" && req.mode != "migrate" {
        return Err(CommandError::new(
            "PRECHECK_INVALID_MODE",
            "mode must be either 'bootstrap' or 'migrate'.",
            trace_id,
            false,
            json!({ "mode": req.mode }),
        ));
    }

    let profile =
        match profiles::profile_by_id(&req.app_id).map_err(|err| profile_error(&trace_id, &err))? {
            Some(profile) => profile,
            None => {
                return Err(CommandError::new(
                    "PRECHECK_PROFILE_NOT_FOUND",
                    "No profile found for app_id.",
                    trace_id,
                    false,
                    json!({ "app_id": req.app_id }),
                ))
            }
        };

    if profile.tier == "blocked" {
        return Err(CommandError::new(
            "PRECHECK_TIER_BLOCKED",
            "Blocked profile cannot be migrated.",
            trace_id,
            false,
            json!({ "app_id": profile.app_id }),
        ));
    }

    if profile.tier == "experimental" && !req.allow_experimental {
        return Err(CommandError::new(
            "PRECHECK_EXPERIMENTAL_NOT_CONFIRMED",
            "Experimental profile requires explicit confirmation.",
            trace_id,
            true,
            json!({ "app_id": profile.app_id }),
        ));
    }

    if req.mode == "bootstrap" && !profile.precheck_rules.allow_bootstrap_if_source_missing {
        return Err(CommandError::new(
            "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
            "This profile does not allow bootstrap mode.",
            trace_id,
            false,
            json!({ "app_id": profile.app_id }),
        ));
    }

    let source_path = profile
        .source_paths
        .first()
        .map(|path| expand_tilde(path))
        .ok_or_else(|| {
            CommandError::new(
                "PRECHECK_SOURCE_NOT_FOUND",
                "No source path defined for profile.",
                trace_id.clone(),
                false,
                json!({ "app_id": profile.app_id }),
            )
        })?;

    let relocation_id = new_relocation_id();
    let target_path = target_path(&profile, &req.target_root);
    let backup_path = if req.mode == "migrate" {
        Some(format!("{source_path}.bak"))
    } else {
        None
    };

    let now = now_iso();
    state
        .db
        .insert_relocation(&NewRelocationRecord {
            relocation_id: relocation_id.clone(),
            app_id: profile.app_id.clone(),
            tier: profile.tier.clone(),
            mode: req.mode.clone(),
            source_path: source_path.clone(),
            target_root: req.target_root.clone(),
            target_path: target_path.clone(),
            backup_path: backup_path.clone(),
            state: "PRECHECKING".to_string(),
            health_state: "unknown".to_string(),
            last_error_code: None,
            trace_id: trace_id.clone(),
            source_size_bytes: 0,
            target_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        })
        .map_err(|err| db_error(&trace_id, "insert relocation", err))?;

    run_precheck_step_guarded(&state, &relocation_id, &trace_id, "process_stopped", || {
        let running = detect_running_processes(&profile);
        if !running.is_empty() {
            return Err(precheck_error(
                "PRECHECK_PROCESS_RUNNING",
                "target app process is still running.",
                &trace_id,
                true,
                json!({ "running_processes": running }),
            ));
        }
        Ok(json!({ "required_processes": profile.process_names, "running_processes": [] }))
    })?;

    let mut source_size_bytes = 0_u64;
    run_precheck_step_guarded(&state, &relocation_id, &trace_id, "source_ready", || {
        source_size_bytes = check_source(
            &req.mode,
            &source_path,
            profile.precheck_rules.allow_bootstrap_if_source_missing,
            profile.precheck_rules.require_full_disk_access,
            &trace_id,
        )?;
        Ok(json!({
          "source_path": source_path,
          "source_size_bytes": source_size_bytes,
          "mode": req.mode
        }))
    })?;

    if req.mode == "bootstrap" {
        run_precheck_step_guarded(
            &state,
            &relocation_id,
            &trace_id,
            "bootstrap_source_empty",
            || {
                if source_size_bytes > 0 {
                    return Err(precheck_error(
                        "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
                        "bootstrap mode requires source path without existing data.",
                        &trace_id,
                        false,
                        json!({
                          "source_path": source_path,
                          "source_size_bytes": source_size_bytes
                        }),
                    ));
                }
                Ok(json!({ "source_size_bytes": source_size_bytes }))
            },
        )?;
    }

    state
        .db
        .update_relocation_sizes(&relocation_id, source_size_bytes as i64, 0, &now_iso())
        .map_err(|err| db_error(&trace_id, "update relocation size", err))?;

    run_precheck_step_guarded(&state, &relocation_id, &trace_id, "target_online", || {
        check_target_online(&req.target_root, &trace_id)?;
        Ok(json!({ "target_root": req.target_root, "online": true }))
    })?;

    run_precheck_step_guarded(&state, &relocation_id, &trace_id, "target_writable", || {
        check_target_writable(&req.target_root, &trace_id)?;
        Ok(json!({ "target_root": req.target_root, "writable": true }))
    })?;

    run_precheck_step_guarded(&state, &relocation_id, &trace_id, "space_budget", || {
        let (free_bytes, required_bytes) =
            check_available_space(&req.target_root, source_size_bytes, &trace_id)?;
        Ok(json!({
          "target_root": req.target_root,
          "free_bytes": free_bytes,
          "required_bytes": required_bytes,
          "source_size_bytes": source_size_bytes
        }))
    })?;

    if req.mode == "bootstrap" {
        state
            .db
            .update_relocation_state(
                &relocation_id,
                "BOOTSTRAP_INIT",
                "unknown",
                &trace_id,
                None,
                &now_iso(),
                None,
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "bootstrap_switch",
            "started",
            None,
            None,
            "bootstrap switch started",
            json!({ "source_path": source_path, "target_path": target_path }),
        )?;

        let bootstrap_timer = Instant::now();
        let switch_outcome =
            match execute_bootstrap_switch(Path::new(&source_path), Path::new(&target_path)) {
                Ok(outcome) => {
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "bootstrap_switch",
                        "succeeded",
                        None,
                        Some(bootstrap_timer.elapsed().as_millis() as i64),
                        "bootstrap switch succeeded",
                        json!({
                          "source_placeholder_removed": outcome.source_placeholder_removed,
                          "target_dir_created": outcome.target_dir_created
                        }),
                    )?;
                    outcome
                }
                Err(err) => {
                    let command_err = bootstrap_error(&trace_id, err);
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "bootstrap_switch",
                        "failed",
                        Some(&command_err.code),
                        Some(bootstrap_timer.elapsed().as_millis() as i64),
                        &command_err.message,
                        command_err.details.clone(),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLED_BACK",
                            "unknown",
                            &trace_id,
                            Some(&command_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|update_err| {
                            db_error(&trace_id, "update relocation state", update_err)
                        })?;
                    return Err(command_err);
                }
            };

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "bootstrap_postcheck",
            "started",
            None,
            None,
            "bootstrap postcheck started",
            json!({}),
        )?;

        let postcheck_timer = Instant::now();
        if let Err(postcheck_err) = postcheck_symlink_target(&source_path, &target_path, &trace_id)
        {
            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "migration",
                "bootstrap_postcheck",
                "failed",
                Some(&postcheck_err.code),
                Some(postcheck_timer.elapsed().as_millis() as i64),
                &postcheck_err.message,
                postcheck_err.details.clone(),
            )?;

            state
                .db
                .update_relocation_state(
                    &relocation_id,
                    "ROLLING_BACK",
                    "broken",
                    &trace_id,
                    Some(&postcheck_err.code),
                    &now_iso(),
                    None,
                )
                .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "rollback",
                "bootstrap_cleanup",
                "started",
                None,
                None,
                "bootstrap rollback cleanup started",
                json!({}),
            )?;

            let rollback_timer = Instant::now();
            match rollback_bootstrap_switch(
                Path::new(&source_path),
                Path::new(&target_path),
                &switch_outcome,
            ) {
                Ok(()) => {
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "bootstrap_cleanup",
                        "succeeded",
                        None,
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        "bootstrap rollback cleanup succeeded",
                        json!({}),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLED_BACK",
                            "unknown",
                            &trace_id,
                            Some(&postcheck_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
                    return Err(postcheck_err);
                }
                Err(rollback_err) => {
                    let rollback_command_err = bootstrap_error(&trace_id, rollback_err);
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "bootstrap_cleanup",
                        "failed",
                        Some(&rollback_command_err.code),
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        &rollback_command_err.message,
                        rollback_command_err.details.clone(),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLBACK_FAILED",
                            "broken",
                            &trace_id,
                            Some(&rollback_command_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
                    return Err(rollback_command_err);
                }
            }
        } else {
            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "migration",
                "bootstrap_postcheck",
                "succeeded",
                None,
                Some(postcheck_timer.elapsed().as_millis() as i64),
                "bootstrap postcheck succeeded",
                json!({ "source_path": source_path, "target_path": target_path }),
            )?;
        }

        let completed_at = now_iso();
        state
            .db
            .update_relocation_state(
                &relocation_id,
                "HEALTHY",
                "healthy",
                &trace_id,
                None,
                &completed_at,
                Some(&completed_at),
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "metadata_commit",
            "succeeded",
            None,
            Some(0),
            "bootstrap mode committed.",
            json!({ "state": "HEALTHY", "health_state": "healthy", "mode": "bootstrap" }),
        )?;

        state
            .db
            .insert_health_snapshot(&NewHealthSnapshot {
                snapshot_id: new_snapshot_id(),
                relocation_id: relocation_id.clone(),
                state: "healthy".to_string(),
                check_code: "HEALTH_BOOTSTRAP_OK".to_string(),
                details_json: json!({ "message": "Bootstrap switch and postcheck succeeded." })
                    .to_string(),
                observed_at: now_iso(),
            })
            .map_err(|err| db_error(&trace_id, "insert health snapshot", err))?;
    } else {
        let backup_path_value = backup_path.clone().ok_or_else(|| {
            CommandError::new(
                "DB_OPERATION_FAILED",
                "backup_path missing for migrate mode.",
                trace_id.clone(),
                false,
                json!({ "mode": "migrate" }),
            )
        })?;
        let temp_path = format!("{}.tmp.{}", target_path, relocation_id);

        run_precheck_step_guarded(
            &state,
            &relocation_id,
            &trace_id,
            "migrate_target_absent",
            || {
                if Path::new(&target_path).exists() {
                    return Err(precheck_error(
                        "PRECHECK_TARGET_PATH_EXISTS",
                        "target path already exists.",
                        &trace_id,
                        false,
                        json!({ "target_path": target_path }),
                    ));
                }
                if Path::new(&backup_path_value).exists() {
                    return Err(precheck_error(
                        "PRECHECK_BACKUP_PATH_EXISTS",
                        "backup path already exists.",
                        &trace_id,
                        false,
                        json!({ "backup_path": backup_path_value }),
                    ));
                }
                Ok(json!({
                    "target_path": target_path,
                    "backup_path": backup_path_value
                }))
            },
        )?;

        state
            .db
            .update_relocation_state(
                &relocation_id,
                "COPYING",
                "unknown",
                &trace_id,
                None,
                &now_iso(),
                None,
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "copy_to_temp",
            "started",
            None,
            None,
            "copy source -> temp started",
            json!({
              "source_path": source_path,
              "temp_path": temp_path
            }),
        )?;

        let copy_timer = Instant::now();
        match copy_source_to_temp(Path::new(&source_path), Path::new(&temp_path)) {
            Ok(copy_result) => {
                insert_operation_log(
                    &state.db,
                    &trace_id,
                    &relocation_id,
                    "migration",
                    "copy_to_temp",
                    "succeeded",
                    None,
                    Some(copy_timer.elapsed().as_millis() as i64),
                    "copy source -> temp succeeded",
                    json!({ "copied_bytes": copy_result.copied_bytes }),
                )?;
            }
            Err(err) => {
                let command_err = migration_error(&trace_id, err);
                insert_operation_log(
                    &state.db,
                    &trace_id,
                    &relocation_id,
                    "migration",
                    "copy_to_temp",
                    "failed",
                    Some(&command_err.code),
                    Some(copy_timer.elapsed().as_millis() as i64),
                    &command_err.message,
                    command_err.details.clone(),
                )?;

                insert_operation_log(
                    &state.db,
                    &trace_id,
                    &relocation_id,
                    "rollback",
                    "cleanup_temp",
                    "started",
                    None,
                    None,
                    "rollback cleanup temp started",
                    json!({ "temp_path": temp_path }),
                )?;
                let cleanup_timer = Instant::now();
                match cleanup_temp_path(Path::new(&temp_path)) {
                    Ok(()) => {
                        insert_operation_log(
                            &state.db,
                            &trace_id,
                            &relocation_id,
                            "rollback",
                            "cleanup_temp",
                            "succeeded",
                            None,
                            Some(cleanup_timer.elapsed().as_millis() as i64),
                            "rollback cleanup temp succeeded",
                            json!({ "temp_path": temp_path }),
                        )?;
                        let completed_at = now_iso();
                        state
                            .db
                            .update_relocation_state(
                                &relocation_id,
                                "ROLLED_BACK",
                                "unknown",
                                &trace_id,
                                Some(&command_err.code),
                                &completed_at,
                                Some(&completed_at),
                            )
                            .map_err(|update_err| {
                                db_error(&trace_id, "update relocation state", update_err)
                            })?;
                        return Err(command_err);
                    }
                    Err(cleanup_err) => {
                        let cleanup_command_err = migration_error(&trace_id, cleanup_err);
                        insert_operation_log(
                            &state.db,
                            &trace_id,
                            &relocation_id,
                            "rollback",
                            "cleanup_temp",
                            "failed",
                            Some(&cleanup_command_err.code),
                            Some(cleanup_timer.elapsed().as_millis() as i64),
                            &cleanup_command_err.message,
                            cleanup_command_err.details.clone(),
                        )?;
                        let completed_at = now_iso();
                        state
                            .db
                            .update_relocation_state(
                                &relocation_id,
                                "ROLLBACK_FAILED",
                                "broken",
                                &trace_id,
                                Some(&cleanup_command_err.code),
                                &completed_at,
                                Some(&completed_at),
                            )
                            .map_err(|update_err| {
                                db_error(&trace_id, "update relocation state", update_err)
                            })?;
                        return Err(cleanup_command_err);
                    }
                }
            }
        }

        state
            .db
            .update_relocation_state(
                &relocation_id,
                "VERIFYING",
                "unknown",
                &trace_id,
                None,
                &now_iso(),
                None,
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "verify_copy",
            "started",
            None,
            None,
            "verify source/temp started",
            json!({ "source_path": source_path, "temp_path": temp_path }),
        )?;

        let verify_timer = Instant::now();
        let verify_result =
            match verify_source_and_temp(Path::new(&source_path), Path::new(&temp_path)) {
                Ok(result) => {
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "verify_copy",
                        "succeeded",
                        None,
                        Some(verify_timer.elapsed().as_millis() as i64),
                        "verify source/temp succeeded",
                        json!({
                          "source_size_bytes": result.source_size_bytes,
                          "temp_size_bytes": result.temp_size_bytes
                        }),
                    )?;
                    result
                }
                Err(err) => {
                    let command_err = migration_error(&trace_id, err);
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "verify_copy",
                        "failed",
                        Some(&command_err.code),
                        Some(verify_timer.elapsed().as_millis() as i64),
                        &command_err.message,
                        command_err.details.clone(),
                    )?;

                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "cleanup_temp",
                        "started",
                        None,
                        None,
                        "rollback cleanup temp started",
                        json!({ "temp_path": temp_path }),
                    )?;
                    let cleanup_timer = Instant::now();
                    match cleanup_temp_path(Path::new(&temp_path)) {
                        Ok(()) => {
                            insert_operation_log(
                                &state.db,
                                &trace_id,
                                &relocation_id,
                                "rollback",
                                "cleanup_temp",
                                "succeeded",
                                None,
                                Some(cleanup_timer.elapsed().as_millis() as i64),
                                "rollback cleanup temp succeeded",
                                json!({ "temp_path": temp_path }),
                            )?;
                            let completed_at = now_iso();
                            state
                                .db
                                .update_relocation_state(
                                    &relocation_id,
                                    "ROLLED_BACK",
                                    "unknown",
                                    &trace_id,
                                    Some(&command_err.code),
                                    &completed_at,
                                    Some(&completed_at),
                                )
                                .map_err(|update_err| {
                                    db_error(&trace_id, "update relocation state", update_err)
                                })?;
                            return Err(command_err);
                        }
                        Err(cleanup_err) => {
                            let cleanup_command_err = migration_error(&trace_id, cleanup_err);
                            insert_operation_log(
                                &state.db,
                                &trace_id,
                                &relocation_id,
                                "rollback",
                                "cleanup_temp",
                                "failed",
                                Some(&cleanup_command_err.code),
                                Some(cleanup_timer.elapsed().as_millis() as i64),
                                &cleanup_command_err.message,
                                cleanup_command_err.details.clone(),
                            )?;
                            let completed_at = now_iso();
                            state
                                .db
                                .update_relocation_state(
                                    &relocation_id,
                                    "ROLLBACK_FAILED",
                                    "broken",
                                    &trace_id,
                                    Some(&cleanup_command_err.code),
                                    &completed_at,
                                    Some(&completed_at),
                                )
                                .map_err(|update_err| {
                                    db_error(&trace_id, "update relocation state", update_err)
                                })?;
                            return Err(cleanup_command_err);
                        }
                    }
                }
            };

        state
            .db
            .update_relocation_sizes(
                &relocation_id,
                verify_result.source_size_bytes as i64,
                verify_result.temp_size_bytes as i64,
                &now_iso(),
            )
            .map_err(|err| db_error(&trace_id, "update relocation size", err))?;

        state
            .db
            .update_relocation_state(
                &relocation_id,
                "SWITCHING",
                "unknown",
                &trace_id,
                None,
                &now_iso(),
                None,
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "switch_paths",
            "started",
            None,
            None,
            "switch paths started",
            json!({
              "source_path": source_path,
              "temp_path": temp_path,
              "target_path": target_path,
              "backup_path": backup_path_value
            }),
        )?;

        let switch_timer = Instant::now();
        if let Err(err) = switch_to_symlink(
            Path::new(&source_path),
            Path::new(&temp_path),
            Path::new(&target_path),
            Path::new(&backup_path_value),
        ) {
            let command_err = migration_error(&trace_id, err);
            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "migration",
                "switch_paths",
                "failed",
                Some(&command_err.code),
                Some(switch_timer.elapsed().as_millis() as i64),
                &command_err.message,
                command_err.details.clone(),
            )?;

            state
                .db
                .update_relocation_state(
                    &relocation_id,
                    "ROLLING_BACK",
                    "broken",
                    &trace_id,
                    Some(&command_err.code),
                    &now_iso(),
                    None,
                )
                .map_err(|update_err| db_error(&trace_id, "update relocation state", update_err))?;

            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "rollback",
                "migrate_cleanup",
                "started",
                None,
                None,
                "migrate rollback cleanup started",
                json!({}),
            )?;

            let rollback_timer = Instant::now();
            match rollback_migration_paths(
                Path::new(&source_path),
                Path::new(&temp_path),
                Path::new(&target_path),
                Path::new(&backup_path_value),
            ) {
                Ok(()) => {
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "migrate_cleanup",
                        "succeeded",
                        None,
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        "migrate rollback cleanup succeeded",
                        json!({}),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLED_BACK",
                            "unknown",
                            &trace_id,
                            Some(&command_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|update_err| {
                            db_error(&trace_id, "update relocation state", update_err)
                        })?;
                    return Err(command_err);
                }
                Err(rollback_err) => {
                    let rollback_command_err = migration_error(&trace_id, rollback_err);
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "migrate_cleanup",
                        "failed",
                        Some(&rollback_command_err.code),
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        &rollback_command_err.message,
                        rollback_command_err.details.clone(),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLBACK_FAILED",
                            "broken",
                            &trace_id,
                            Some(&rollback_command_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|update_err| {
                            db_error(&trace_id, "update relocation state", update_err)
                        })?;
                    return Err(rollback_command_err);
                }
            }
        }

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "switch_paths",
            "succeeded",
            None,
            Some(switch_timer.elapsed().as_millis() as i64),
            "switch paths succeeded",
            json!({}),
        )?;

        state
            .db
            .update_relocation_state(
                &relocation_id,
                "POSTCHECKING",
                "unknown",
                &trace_id,
                None,
                &now_iso(),
                None,
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "postcheck",
            "started",
            None,
            None,
            "postcheck started",
            json!({}),
        )?;

        let postcheck_timer = Instant::now();
        if let Err(postcheck_err) = postcheck_symlink_target(&source_path, &target_path, &trace_id)
        {
            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "migration",
                "postcheck",
                "failed",
                Some(&postcheck_err.code),
                Some(postcheck_timer.elapsed().as_millis() as i64),
                &postcheck_err.message,
                postcheck_err.details.clone(),
            )?;

            state
                .db
                .update_relocation_state(
                    &relocation_id,
                    "ROLLING_BACK",
                    "broken",
                    &trace_id,
                    Some(&postcheck_err.code),
                    &now_iso(),
                    None,
                )
                .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "rollback",
                "migrate_cleanup",
                "started",
                None,
                None,
                "migrate rollback cleanup started",
                json!({}),
            )?;

            let rollback_timer = Instant::now();
            match rollback_migration_paths(
                Path::new(&source_path),
                Path::new(&temp_path),
                Path::new(&target_path),
                Path::new(&backup_path_value),
            ) {
                Ok(()) => {
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "migrate_cleanup",
                        "succeeded",
                        None,
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        "migrate rollback cleanup succeeded",
                        json!({}),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLED_BACK",
                            "unknown",
                            &trace_id,
                            Some(&postcheck_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
                    return Err(postcheck_err);
                }
                Err(rollback_err) => {
                    let rollback_command_err = migration_error(&trace_id, rollback_err);
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "rollback",
                        "migrate_cleanup",
                        "failed",
                        Some(&rollback_command_err.code),
                        Some(rollback_timer.elapsed().as_millis() as i64),
                        &rollback_command_err.message,
                        rollback_command_err.details.clone(),
                    )?;
                    let completed_at = now_iso();
                    state
                        .db
                        .update_relocation_state(
                            &relocation_id,
                            "ROLLBACK_FAILED",
                            "broken",
                            &trace_id,
                            Some(&rollback_command_err.code),
                            &completed_at,
                            Some(&completed_at),
                        )
                        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
                    return Err(rollback_command_err);
                }
            }
        }

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "postcheck",
            "succeeded",
            None,
            Some(postcheck_timer.elapsed().as_millis() as i64),
            "postcheck succeeded",
            json!({ "source_path": source_path, "target_path": target_path }),
        )?;

        let mut backup_cleanup_details = json!({
            "enabled": req.cleanup_backup_after_migrate,
            "attempted": false,
            "removed": false
        });

        if req.cleanup_backup_after_migrate {
            insert_operation_log(
                &state.db,
                &trace_id,
                &relocation_id,
                "migration",
                "cleanup_backup",
                "started",
                None,
                None,
                "cleanup backup started",
                json!({ "backup_path": backup_path_value }),
            )?;

            let cleanup_backup_timer = Instant::now();
            match remove_path_if_exists(Path::new(&backup_path_value)) {
                Ok(()) => {
                    backup_cleanup_details = json!({
                        "enabled": true,
                        "attempted": true,
                        "removed": true,
                        "backup_path": backup_path_value
                    });
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "cleanup_backup",
                        "succeeded",
                        None,
                        Some(cleanup_backup_timer.elapsed().as_millis() as i64),
                        "cleanup backup succeeded",
                        backup_cleanup_details.clone(),
                    )?;
                }
                Err(cleanup_err) => {
                    let cleanup_command_err = migration_error(&trace_id, cleanup_err);
                    backup_cleanup_details = json!({
                        "enabled": true,
                        "attempted": true,
                        "removed": false,
                        "backup_path": backup_path_value,
                        "error_code": cleanup_command_err.code,
                        "error": cleanup_command_err.message
                    });
                    insert_operation_log(
                        &state.db,
                        &trace_id,
                        &relocation_id,
                        "migration",
                        "cleanup_backup",
                        "failed",
                        Some(&cleanup_command_err.code),
                        Some(cleanup_backup_timer.elapsed().as_millis() as i64),
                        &cleanup_command_err.message,
                        cleanup_command_err.details.clone(),
                    )?;
                }
            }
        }

        let completed_at = now_iso();
        state
            .db
            .update_relocation_state(
                &relocation_id,
                "HEALTHY",
                "healthy",
                &trace_id,
                None,
                &completed_at,
                Some(&completed_at),
            )
            .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

        insert_operation_log(
            &state.db,
            &trace_id,
            &relocation_id,
            "migration",
            "metadata_commit",
            "succeeded",
            None,
            Some(0),
            "migrate mode committed.",
            json!({
                "state": "HEALTHY",
                "health_state": "healthy",
                "mode": "migrate",
                "cleanup_backup": backup_cleanup_details
            }),
        )?;

        state
            .db
            .insert_health_snapshot(&NewHealthSnapshot {
                snapshot_id: new_snapshot_id(),
                relocation_id: relocation_id.clone(),
                state: "healthy".to_string(),
                check_code: "HEALTH_MIGRATE_OK".to_string(),
                details_json: json!({ "message": "Migrate transaction succeeded." }).to_string(),
                observed_at: now_iso(),
            })
            .map_err(|err| db_error(&trace_id, "insert health snapshot", err))?;
    }

    let saved = state
        .db
        .get_relocation(&relocation_id)
        .map_err(|err| db_error(&trace_id, "read relocation", err))?
        .ok_or_else(|| {
            CommandError::new(
                "DB_OPERATION_FAILED",
                "saved relocation record not found.",
                trace_id.clone(),
                true,
                json!({ "relocation_id": relocation_id }),
            )
        })?;

    Ok(relocation_to_result(saved))
}

#[tauri::command]
pub fn rollback_relocation(
    req: RollbackRequest,
    state: State<'_, AppState>,
) -> Result<RelocationResult, CommandError> {
    let trace_id = new_trace_id();
    let existing = state
        .db
        .get_relocation(&req.relocation_id)
        .map_err(|err| db_error(&trace_id, "read relocation", err))?
        .ok_or_else(|| {
            CommandError::new(
                "ROLLBACK_RELOCATION_NOT_FOUND",
                "relocation_id was not found.",
                trace_id.clone(),
                false,
                json!({ "relocation_id": req.relocation_id }),
            )
        })?;

    let source_path = Path::new(&existing.source_path);
    let target_path = Path::new(&existing.target_path);
    let backup_path = existing.backup_path.clone().map(std::path::PathBuf::from);
    let temp_path = format!("{}.tmp.{}", existing.target_path, existing.relocation_id);

    state
        .db
        .update_relocation_state(
            &req.relocation_id,
            "ROLLING_BACK",
            "unknown",
            &trace_id,
            None,
            &now_iso(),
            None,
        )
        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

    insert_operation_log(
        &state.db,
        &trace_id,
        &req.relocation_id,
        "rollback",
        "remove_source_symlink",
        "started",
        None,
        None,
        "rollback remove source symlink started",
        json!({ "source_path": existing.source_path }),
    )?;

    let remove_timer = Instant::now();
    let remove_symlink_result: Result<serde_json::Value, CommandError> =
        match fs::symlink_metadata(source_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::remove_file(source_path).map_err(|err| {
                    CommandError::new(
                        "ROLLBACK_REMOVE_SYMLINK_FAILED",
                        "failed to remove source symlink.",
                        trace_id.clone(),
                        true,
                        json!({ "source_path": existing.source_path, "error": err.to_string() }),
                    )
                })?;
                Ok(json!({ "removed": true }))
            }
            Ok(_) => Ok(json!({ "removed": false, "reason": "source_not_symlink" })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(json!({ "removed": false, "reason": "source_missing" }))
            }
            Err(err) => Err(CommandError::new(
                "ROLLBACK_REMOVE_SYMLINK_FAILED",
                "failed to inspect source path.",
                trace_id.clone(),
                true,
                json!({ "source_path": existing.source_path, "error": err.to_string() }),
            )),
        };

    let remove_details = match remove_symlink_result {
        Ok(details) => details,
        Err(remove_err) => {
            insert_operation_log(
                &state.db,
                &trace_id,
                &req.relocation_id,
                "rollback",
                "remove_source_symlink",
                "failed",
                Some(&remove_err.code),
                Some(remove_timer.elapsed().as_millis() as i64),
                &remove_err.message,
                remove_err.details.clone(),
            )?;
            let completed_at = now_iso();
            state
                .db
                .update_relocation_state(
                    &req.relocation_id,
                    "ROLLBACK_FAILED",
                    "broken",
                    &trace_id,
                    Some(&remove_err.code),
                    &completed_at,
                    Some(&completed_at),
                )
                .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
            return Err(remove_err);
        }
    };

    insert_operation_log(
        &state.db,
        &trace_id,
        &req.relocation_id,
        "rollback",
        "remove_source_symlink",
        "succeeded",
        None,
        Some(remove_timer.elapsed().as_millis() as i64),
        "rollback remove source symlink finished",
        remove_details,
    )?;

    insert_operation_log(
        &state.db,
        &trace_id,
        &req.relocation_id,
        "rollback",
        "restore_source",
        "started",
        None,
        None,
        "rollback restore source started",
        json!({
            "source_path": existing.source_path,
            "target_path": existing.target_path,
            "backup_path": existing.backup_path
        }),
    )?;

    let restore_timer = Instant::now();
    let restore_result: Result<serde_json::Value, CommandError> = (|| {
        if let Some(backup_path) = backup_path.as_ref() {
            if backup_path.exists() {
                if source_path.exists() {
                    if req.force {
                        remove_path_if_exists(source_path)
                            .map_err(|err| migration_error(&trace_id, err))?;
                    } else {
                        return Err(CommandError::new(
                            "ROLLBACK_RESTORE_BACKUP_FAILED",
                            "source path exists while backup path is present, set force=true to override.",
                            trace_id.clone(),
                            false,
                            json!({
                                "source_path": existing.source_path,
                                "backup_path": backup_path
                            }),
                        ));
                    }
                }

                fs::rename(backup_path, source_path).map_err(|err| {
                    CommandError::new(
                        "ROLLBACK_RESTORE_BACKUP_FAILED",
                        "failed to restore source path from backup path.",
                        trace_id.clone(),
                        false,
                        json!({
                            "source_path": existing.source_path,
                            "backup_path": backup_path,
                            "error": err.to_string()
                        }),
                    )
                })?;
                return Ok(json!({ "method": "backup_rename", "force": req.force }));
            }
        }

        if existing.mode == "bootstrap" {
            if !source_path.exists() {
                if target_path.exists() {
                    match fs::rename(target_path, source_path) {
                        Ok(()) => {
                            return Ok(
                                json!({ "method": "target_rename_to_source", "force": req.force }),
                            )
                        }
                        Err(rename_err) => {
                            copy_path_to_path(target_path, source_path)
                                .map_err(|err| migration_error(&trace_id, err))?;
                            return Ok(json!({
                                "method": "target_copy_to_source",
                                "rename_error": rename_err.to_string()
                            }));
                        }
                    }
                } else {
                    fs::create_dir_all(source_path).map_err(|err| {
                        CommandError::new(
                            "ROLLBACK_RESTORE_BACKUP_FAILED",
                            "failed to recreate source path for bootstrap rollback.",
                            trace_id.clone(),
                            false,
                            json!({ "source_path": existing.source_path, "error": err.to_string() }),
                        )
                    })?;
                    return Ok(json!({ "method": "create_empty_source" }));
                }
            }
            return Ok(json!({ "method": "source_already_present" }));
        }

        if source_path.exists() {
            Ok(json!({ "method": "source_already_present", "backup_found": false }))
        } else if target_path.exists() {
            match fs::rename(target_path, source_path) {
                Ok(()) => Ok(json!({
                    "method": "target_rename_to_source_without_backup",
                    "force": req.force
                })),
                Err(rename_err) => {
                    copy_path_to_path(target_path, source_path)
                        .map_err(|err| migration_error(&trace_id, err))?;
                    Ok(json!({
                        "method": "target_copy_to_source_without_backup",
                        "rename_error": rename_err.to_string()
                    }))
                }
            }
        } else {
            Err(CommandError::new(
                "ROLLBACK_RESTORE_BACKUP_FAILED",
                "source path missing and backup path not found.",
                trace_id.clone(),
                false,
                json!({
                    "source_path": existing.source_path,
                    "target_path": existing.target_path,
                    "backup_path": existing.backup_path
                }),
            ))
        }
    })();

    let restore_details = match restore_result {
        Ok(details) => {
            insert_operation_log(
                &state.db,
                &trace_id,
                &req.relocation_id,
                "rollback",
                "restore_source",
                "succeeded",
                None,
                Some(restore_timer.elapsed().as_millis() as i64),
                "rollback restore source succeeded",
                details.clone(),
            )?;
            details
        }
        Err(restore_err) => {
            insert_operation_log(
                &state.db,
                &trace_id,
                &req.relocation_id,
                "rollback",
                "restore_source",
                "failed",
                Some(&restore_err.code),
                Some(restore_timer.elapsed().as_millis() as i64),
                &restore_err.message,
                restore_err.details.clone(),
            )?;
            let completed_at = now_iso();
            state
                .db
                .update_relocation_state(
                    &req.relocation_id,
                    "ROLLBACK_FAILED",
                    "broken",
                    &trace_id,
                    Some(&restore_err.code),
                    &completed_at,
                    Some(&completed_at),
                )
                .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
            return Err(restore_err);
        }
    };

    insert_operation_log(
        &state.db,
        &trace_id,
        &req.relocation_id,
        "rollback",
        "cleanup_temp",
        "started",
        None,
        None,
        "rollback cleanup temp started",
        json!({ "temp_path": temp_path }),
    )?;

    let cleanup_timer = Instant::now();
    match cleanup_temp_path(Path::new(&temp_path)) {
        Ok(()) => {
            insert_operation_log(
                &state.db,
                &trace_id,
                &req.relocation_id,
                "rollback",
                "cleanup_temp",
                "succeeded",
                None,
                Some(cleanup_timer.elapsed().as_millis() as i64),
                "rollback cleanup temp succeeded",
                json!({ "temp_path": temp_path }),
            )?;
        }
        Err(cleanup_err) => {
            let cleanup_command_err = migration_error(&trace_id, cleanup_err);
            insert_operation_log(
                &state.db,
                &trace_id,
                &req.relocation_id,
                "rollback",
                "cleanup_temp",
                "failed",
                Some(&cleanup_command_err.code),
                Some(cleanup_timer.elapsed().as_millis() as i64),
                &cleanup_command_err.message,
                cleanup_command_err.details.clone(),
            )?;
            let completed_at = now_iso();
            state
                .db
                .update_relocation_state(
                    &req.relocation_id,
                    "ROLLBACK_FAILED",
                    "broken",
                    &trace_id,
                    Some(&cleanup_command_err.code),
                    &completed_at,
                    Some(&completed_at),
                )
                .map_err(|err| db_error(&trace_id, "update relocation state", err))?;
            return Err(cleanup_command_err);
        }
    }

    let completed_at = now_iso();
    state
        .db
        .update_relocation_state(
            &req.relocation_id,
            "ROLLED_BACK",
            "healthy",
            &trace_id,
            None,
            &completed_at,
            Some(&completed_at),
        )
        .map_err(|err| db_error(&trace_id, "update relocation state", err))?;

    insert_operation_log(
        &state.db,
        &trace_id,
        &req.relocation_id,
        "rollback",
        "state_restore",
        "succeeded",
        None,
        Some(0),
        "Rollback state committed.",
        json!({
            "force": req.force,
            "previous_state": existing.state,
            "restore": restore_details
        }),
    )?;

    state
        .db
        .insert_health_snapshot(&NewHealthSnapshot {
            snapshot_id: new_snapshot_id(),
            relocation_id: req.relocation_id.clone(),
            state: "healthy".to_string(),
            check_code: "HEALTH_ROLLBACK_OK".to_string(),
            details_json: json!({
                "message": "Rollback completed with source restoration.",
                "restore": restore_details
            })
            .to_string(),
            observed_at: completed_at.clone(),
        })
        .map_err(|err| db_error(&trace_id, "insert health snapshot", err))?;

    let updated = state
        .db
        .get_relocation(&req.relocation_id)
        .map_err(|err| db_error(&trace_id, "read relocation", err))?
        .ok_or_else(|| {
            CommandError::new(
                "DB_OPERATION_FAILED",
                "updated relocation record not found.",
                trace_id.clone(),
                true,
                json!({ "relocation_id": req.relocation_id }),
            )
        })?;

    Ok(relocation_to_result(updated))
}

#[tauri::command]
pub fn export_operation_logs(
    req: ExportLogsRequest,
    state: State<'_, AppState>,
) -> Result<ExportLogsResult, CommandError> {
    let trace_id = new_trace_id();
    let relocation_filter = req.relocation_id.clone();
    let trace_filter = req.trace_id.clone();
    let logs = state
        .db
        .list_operation_logs(relocation_filter.as_deref(), trace_filter.as_deref())
        .map_err(|err| db_error(&trace_id, "list operation logs", err))?;
    let items: Vec<OperationLogItem> = logs.into_iter().map(operation_log_to_item).collect();

    let output_path = req
        .output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_export_output_path(&state.db, &trace_id));

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CommandError::new(
                "LOG_EXPORT_WRITE_FAILED",
                "failed to create export directory.",
                trace_id.clone(),
                true,
                json!({ "output_dir": parent.to_string_lossy().to_string(), "error": err.to_string() }),
            )
        })?;
    }

    let payload = json!({
        "export_trace_id": trace_id,
        "relocation_id": relocation_filter.clone(),
        "trace_id": trace_filter.clone(),
        "exported_at": now_iso(),
        "exported_count": items.len(),
        "operation_logs": items
    });

    let serialized = serde_json::to_string_pretty(&payload).map_err(|err| {
        CommandError::new(
            "LOG_EXPORT_WRITE_FAILED",
            "failed to encode operation logs export payload.",
            trace_id.clone(),
            false,
            json!({ "error": err.to_string() }),
        )
    })?;

    fs::write(&output_path, serialized).map_err(|err| {
        CommandError::new(
            "LOG_EXPORT_WRITE_FAILED",
            "failed to write operation logs export file.",
            trace_id.clone(),
            true,
            json!({ "output_path": output_path.to_string_lossy().to_string(), "error": err.to_string() }),
        )
    })?;

    Ok(ExportLogsResult {
        export_trace_id: trace_id,
        relocation_id: relocation_filter,
        trace_id: trace_filter,
        output_path: output_path.to_string_lossy().to_string(),
        exported_count: items.len(),
    })
}

#[tauri::command]
pub fn list_relocations(
    state: State<'_, AppState>,
) -> Result<Vec<RelocationSummary>, CommandError> {
    let trace_id = new_trace_id();
    let records = state
        .db
        .list_relocations()
        .map_err(|err| db_error(&trace_id, "list relocations", err))?;
    Ok(records
        .into_iter()
        .map(|row| RelocationSummary {
            relocation_id: row.relocation_id,
            app_id: row.app_id,
            state: row.state,
            health_state: row.health_state,
            source_path: row.source_path,
            target_path: row.target_path,
            updated_at: row.updated_at,
        })
        .collect())
}

#[tauri::command]
pub fn list_health_events(
    req: Option<HealthEventsRequest>,
    state: State<'_, AppState>,
) -> Result<Vec<HealthEvent>, CommandError> {
    let trace_id = new_trace_id();
    let requested_limit = req.and_then(|v| v.limit).unwrap_or(200) as usize;
    let limit = requested_limit.clamp(1, 1000);
    let rows = state
        .db
        .list_health_events(limit)
        .map_err(|err| db_error(&trace_id, "list health events", err))?;
    Ok(rows.into_iter().map(health_event_to_model).collect())
}

#[tauri::command]
pub fn reconcile_relocations(
    req: Option<ReconcileRequest>,
    state: State<'_, AppState>,
) -> Result<ReconcileResult, CommandError> {
    let trace_id = new_trace_id();
    let apply_safe_fixes = req
        .as_ref()
        .and_then(|v| v.apply_safe_fixes)
        .unwrap_or(false);
    let requested_limit = req.and_then(|v| v.limit).unwrap_or(500) as usize;
    let limit = requested_limit.clamp(1, 5000);
    reconcile::run_reconcile(&state.db, &trace_id, apply_safe_fixes, limit, true).map_err(|err| {
        CommandError::new(
            "RECONCILE_RUN_FAILED",
            "failed to run reconcile task.",
            trace_id,
            true,
            json!({ "error": err }),
        )
    })
}

#[tauri::command]
pub fn check_health(state: State<'_, AppState>) -> Result<Vec<HealthStatus>, CommandError> {
    let trace_id = new_trace_id();
    health::run_health_check(&state.db, &trace_id, true).map_err(|err| {
        CommandError::new(
            "HEALTH_CHECK_FAILED",
            "failed to run health checks.",
            trace_id,
            true,
            json!({ "error": err }),
        )
    })
}
