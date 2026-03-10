use crate::db::{
    Database, HealthEventRecord, NewHealthSnapshot, NewOperationLogEntry, NewRelocationRecord,
    OperationLogRecord,
};
use crate::models::{
    AppScanPath, AppScanResult, CommandError, DiskStatus, HealthEvent, HealthEventsRequest,
    HealthStatus, MigrateRequest, OperationLogItem, OperationLogsRequest, ReconcileRequest,
    ReconcileResult, RelocationResult, RelocationSummary, RollbackRequest,
};
use crate::profiles::{self, AppProfile, RelocationUnit};
use crate::{
    bootstrap::{execute_bootstrap_switch, rollback_bootstrap_switch, BootstrapSwitchError},
    health,
    migration::{
        cleanup_temp_path, copy_path_to_path, copy_source_to_temp, remove_path_if_exists,
        rollback_migration_paths, switch_to_symlink, verify_source_and_temp, MigrationError,
    },
    reconcile, AppState,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use fs2::{available_space, total_space};
use serde_json::json;
use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::{Instant, UNIX_EPOCH};
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

#[derive(Debug, Clone)]
struct MigrationUnitPlan {
    unit_id: String,
    display_name: String,
    source_path: String,
    target_path_template: String,
    default_enabled: bool,
    enabled: bool,
    risk_level: String,
    requires_confirmation: bool,
    blocked_reason: Option<String>,
    allow_bootstrap_if_source_missing: bool,
    category: String,
}

fn render_target_path(template: &str, target_root: &str, fallback_display_name: &str) -> String {
    if template.trim().is_empty() {
        return format!("{target_root}/AppData/{fallback_display_name}");
    }
    template.replace("{target_root}", target_root)
}

fn normalize_profile_unit(index: usize, raw_unit: &RelocationUnit) -> Option<MigrationUnitPlan> {
    let source_path = raw_unit.source_path.trim();
    if source_path.is_empty() {
        return None;
    }

    let unit_id = if raw_unit.unit_id.trim().is_empty() {
        format!("unit-{}", index + 1)
    } else {
        raw_unit.unit_id.trim().to_string()
    };

    let display_name = if raw_unit.display_name.trim().is_empty() {
        unit_id.clone()
    } else {
        raw_unit.display_name.trim().to_string()
    };

    Some(MigrationUnitPlan {
        unit_id,
        display_name,
        source_path: expand_tilde(source_path),
        target_path_template: raw_unit.target_path_template.clone(),
        default_enabled: raw_unit.default_enabled,
        enabled: raw_unit.enabled,
        risk_level: raw_unit.risk_level.clone(),
        requires_confirmation: raw_unit.requires_confirmation,
        blocked_reason: raw_unit.blocked_reason.clone(),
        allow_bootstrap_if_source_missing: raw_unit.allow_bootstrap_if_source_missing,
        category: raw_unit.category.clone(),
    })
}

fn profile_relocation_units(profile: &AppProfile) -> Vec<MigrationUnitPlan> {
    let units = profile
        .relocation_units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| normalize_profile_unit(index, unit))
        .collect::<Vec<_>>();

    let mut unique = Vec::new();
    for unit in units {
        if unique
            .iter()
            .any(|item: &MigrationUnitPlan| item.unit_id == unit.unit_id)
        {
            continue;
        }
        unique.push(unit);
    }
    unique
}

fn source_path_exists(path: &str) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn pick_default_unit(units: &[MigrationUnitPlan]) -> Option<MigrationUnitPlan> {
    if let Some(unit) = units
        .iter()
        .find(|unit| unit.enabled && unit.default_enabled && source_path_exists(&unit.source_path))
    {
        return Some(unit.clone());
    }
    if let Some(unit) = units
        .iter()
        .find(|unit| unit.enabled && unit.default_enabled)
    {
        return Some(unit.clone());
    }
    if let Some(unit) = units
        .iter()
        .find(|unit| unit.enabled && source_path_exists(&unit.source_path))
    {
        return Some(unit.clone());
    }
    units.iter().find(|unit| unit.enabled).cloned()
}

fn pick_relocation_unit(profile: &AppProfile, unit_id: Option<&str>) -> Option<MigrationUnitPlan> {
    let units = profile_relocation_units(profile);
    if units.is_empty() {
        return None;
    }

    if let Some(unit_id) = unit_id {
        return units
            .into_iter()
            .find(|unit| unit.unit_id == unit_id.trim());
    }

    pick_default_unit(&units)
}

fn target_path_for_unit(unit: &MigrationUnitPlan, target_root: &str) -> String {
    render_target_path(&unit.target_path_template, target_root, &unit.display_name)
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

#[allow(clippy::too_many_arguments)]
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

#[derive(Debug, Clone)]
struct InstalledBundle {
    path: PathBuf,
    stem_lc: String,
    bundle_id_lc: Option<String>,
}

fn list_installed_app_bundles() -> Vec<InstalledBundle> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }

    let mut bundles = Vec::new();
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
                let bundle_id_lc =
                    read_plist_string(&path.join("Contents/Info.plist"), "CFBundleIdentifier")
                        .map(|value| value.to_ascii_lowercase());
                bundles.push(InstalledBundle {
                    path: path.clone(),
                    stem_lc: stem.to_ascii_lowercase(),
                    bundle_id_lc,
                });
            }
        }
    }
    bundles
}

fn profile_match_hints(profile: &AppProfile) -> Vec<String> {
    let mut hints: Vec<String> = profile
        .process_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    hints.push(profile.display_name.to_ascii_lowercase());
    hints.push(profile.app_id.to_ascii_lowercase());
    hints.extend(
        profile
            .bundle_ids
            .iter()
            .map(|item| item.to_ascii_lowercase()),
    );

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
    hints
}

fn match_profile_bundle(
    profile: &AppProfile,
    installed_bundles: &[InstalledBundle],
) -> Option<PathBuf> {
    if installed_bundles.is_empty() {
        return None;
    }

    if !profile.bundle_ids.is_empty() {
        let target_bundle_ids = profile
            .bundle_ids
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !target_bundle_ids.is_empty() {
            if let Some(bundle) = installed_bundles.iter().find(|bundle| {
                bundle
                    .bundle_id_lc
                    .as_ref()
                    .map(|bundle_id| target_bundle_ids.iter().any(|target| target == bundle_id))
                    .unwrap_or(false)
            }) {
                return Some(bundle.path.clone());
            }
        }
    }

    let hints = profile_match_hints(profile);

    installed_bundles
        .iter()
        .find(|bundle| {
            hints.iter().any(|hint| {
                let trimmed = hint.trim();
                !trimmed.is_empty()
                    && (bundle.stem_lc == trimmed
                        || bundle.stem_lc.contains(trimmed)
                        || trimmed.contains(&bundle.stem_lc))
            })
        })
        .map(|bundle| bundle.path.clone())
}

fn resolve_profile_display_name(profile: &AppProfile, bundle_path: Option<&Path>) -> String {
    bundle_path
        .and_then(resolve_bundle_display_name)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.display_name.clone())
}

fn resolve_profile_icon_path(bundle_path: Option<&Path>) -> Option<String> {
    bundle_path.and_then(resolve_bundle_icon_path)
}

fn read_plist_string(plist_path: &Path, key: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg(format!("Print :{key}"))
            .arg(plist_path)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return None;
        }
        Some(value)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (plist_path, key);
        None
    }
}

fn resolve_bundle_display_name(bundle_path: &Path) -> Option<String> {
    let info_plist = bundle_path.join("Contents/Info.plist");
    if info_plist.is_file() {
        if let Some(value) = read_plist_string(&info_plist, "CFBundleDisplayName") {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
        if let Some(value) = read_plist_string(&info_plist, "CFBundleName") {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }

    bundle_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
}

fn resolve_bundle_icon_path(bundle_path: &Path) -> Option<String> {
    let resources = bundle_path.join("Contents/Resources");
    if !resources.is_dir() {
        return None;
    }

    let info_plist = bundle_path.join("Contents/Info.plist");
    if info_plist.is_file() {
        if let Some(icon_file) = read_plist_string(&info_plist, "CFBundleIconFile") {
            let icon_name = icon_file.trim().trim_matches('"').to_string();
            if !icon_name.is_empty() {
                let mut candidates = vec![icon_name.clone()];
                if !icon_name.to_ascii_lowercase().ends_with(".icns") {
                    candidates.push(format!("{icon_name}.icns"));
                }
                for candidate in candidates {
                    let candidate_path = resources.join(candidate);
                    if candidate_path.is_file() {
                        return resolve_web_icon_path(&candidate_path);
                    }
                }
            }
        }
    }

    let mut icns_files = fs::read_dir(&resources)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("icns"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if icns_files.is_empty() {
        return None;
    }

    icns_files.sort();

    if let Some(app_icon) = icns_files.iter().find(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.to_ascii_lowercase().contains("appicon"))
            .unwrap_or(false)
    }) {
        return resolve_web_icon_path(app_icon);
    }

    icns_files
        .first()
        .map(|path| path.to_string_lossy().to_string())
        .and_then(|path| resolve_web_icon_path(Path::new(&path)))
}

fn resolve_web_icon_path(path: &Path) -> Option<String> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
    ) {
        return Some(path.to_string_lossy().to_string());
    }

    if ext == "icns" {
        return convert_icns_to_png(path).or_else(|| Some(path.to_string_lossy().to_string()));
    }

    Some(path.to_string_lossy().to_string())
}

fn cache_png_path_for_icns(icns_path: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(icns_path).ok()?;
    let modified = metadata.modified().ok()?;
    let modified_secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();

    let mut hasher = DefaultHasher::new();
    icns_path.to_string_lossy().hash(&mut hasher);
    modified_secs.hash(&mut hasher);
    let digest = hasher.finish();

    let cache_dir = std::env::temp_dir()
        .join("disk-relocator")
        .join("icon-cache");
    fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir.join(format!("{digest:016x}.png")))
}

fn convert_icns_to_png(icns_path: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output_path = cache_png_path_for_icns(icns_path)?;
        if output_path.is_file() {
            return Some(output_path.to_string_lossy().to_string());
        }

        let status = Command::new("/usr/bin/sips")
            .arg("-s")
            .arg("format")
            .arg("png")
            .arg(icns_path)
            .arg("--out")
            .arg(&output_path)
            .status()
            .ok()?;

        if !status.success() || !output_path.is_file() {
            return None;
        }
        Some(output_path.to_string_lossy().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = icns_path;
        None
    }
}

fn icon_mime_from_ext(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "png" => "image/png",
        _ => "image/png",
    }
}

fn read_icon_data_url(icon_path: &str) -> Option<String> {
    let path = Path::new(icon_path);
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let mime = icon_mime_from_ext(path);
    let payload = BASE64_STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{payload}"))
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

fn mount_root_from_target_root(target_root: &str) -> Option<String> {
    let trimmed = target_root.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "/" {
        return Some("/".to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("/Volumes/") {
        let volume_name = rest.split('/').find(|segment| !segment.is_empty())?;
        return Some(format!("/Volumes/{volume_name}"));
    }

    if trimmed == "/Volumes" || trimmed == "/Volumes/" {
        return None;
    }

    Some(trimmed.to_string())
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

    if !metadata.is_dir() {
        return Err(precheck_error(
            "PRECHECK_SOURCE_NOT_FOUND",
            "source path is not a directory.",
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

    let parent = target.parent().ok_or_else(|| {
        precheck_error(
            "PRECHECK_DISK_OFFLINE",
            "target disk is offline or not mounted.",
            trace_id,
            true,
            json!({ "target_root": target_root }),
        )
    })?;

    // Avoid accidentally creating pseudo mount points directly under /Volumes.
    if !parent.is_dir() || parent == Path::new("/Volumes") {
        return Err(precheck_error(
            "PRECHECK_DISK_OFFLINE",
            "target disk is offline or not mounted.",
            trace_id,
            true,
            json!({ "target_root": target_root }),
        ));
    }

    fs::create_dir_all(target).map_err(|err| {
        let code = if err.kind() == std::io::ErrorKind::PermissionDenied {
            "PRECHECK_DISK_READONLY"
        } else {
            "PRECHECK_DISK_OFFLINE"
        };
        precheck_error(
            code,
            "failed to create target root directory.",
            trace_id,
            true,
            json!({ "target_root": target_root, "error": err.to_string() }),
        )
    })?;

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
    let installed_bundles = list_installed_app_bundles();
    let now = now_iso();
    let results = profiles
        .into_iter()
        .filter_map(|profile| {
            let running = !detect_running_processes(&profile).is_empty();
            let matched_bundle = match_profile_bundle(&profile, &installed_bundles);
            let matched_bundle_path = matched_bundle.as_deref();
            let installed = matched_bundle.is_some();
            let relocation_units = profile_relocation_units(&profile);
            let mut detected_any_path = false;
            let detected_paths = relocation_units
                .iter()
                .map(|unit| {
                    let source = Path::new(&unit.source_path);
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
                                unit_id: Some(unit.unit_id.clone()),
                                display_name: Some(unit.display_name.clone()),
                                default_enabled: Some(unit.default_enabled),
                                enabled: Some(unit.enabled),
                                risk_level: Some(unit.risk_level.clone()),
                                requires_confirmation: Some(unit.requires_confirmation),
                                blocked_reason: unit.blocked_reason.clone(),
                                allow_bootstrap_if_source_missing: Some(
                                    unit.allow_bootstrap_if_source_missing,
                                ),
                                category: Some(unit.category.clone()),
                                path: unit.source_path.clone(),
                                exists: true,
                                is_symlink,
                                size_bytes,
                            }
                        }
                        Err(_) => AppScanPath {
                            unit_id: Some(unit.unit_id.clone()),
                            display_name: Some(unit.display_name.clone()),
                            default_enabled: Some(unit.default_enabled),
                            enabled: Some(unit.enabled),
                            risk_level: Some(unit.risk_level.clone()),
                            requires_confirmation: Some(unit.requires_confirmation),
                            blocked_reason: unit.blocked_reason.clone(),
                            allow_bootstrap_if_source_missing: Some(
                                unit.allow_bootstrap_if_source_missing,
                            ),
                            category: Some(unit.category.clone()),
                            path: unit.source_path.clone(),
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

            let display_name = resolve_profile_display_name(&profile, matched_bundle_path);
            let icon_path = resolve_profile_icon_path(matched_bundle_path);
            let icon_data_url = icon_path.as_ref().and_then(|path| read_icon_data_url(path));

            Some(AppScanResult {
                app_id: profile.app_id,
                display_name,
                description_i18n: profile.description_i18n.clone(),
                icon_path,
                icon_data_url,
                availability: profile.availability.clone(),
                blocked_reason: profile.blocked_reason.clone(),
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
        if let Some(root) = mount_root_from_target_root(&row.target_root) {
            roots.insert(root);
        }
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
pub fn get_system_disk_status() -> Result<DiskStatus, CommandError> {
    let trace_id = new_trace_id();
    let mount_point = "/".to_string();
    let root_path = Path::new(&mount_point);
    let is_mounted = root_path.is_dir();

    let writable_base = std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| root_path.to_path_buf());
    let probe_path = writable_base.join(format!(
        ".disk-relocator-system-disk-probe-{}",
        Uuid::new_v4()
    ));
    let is_writable = match OpenOptions::new()
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
    };

    let free_bytes = available_space(root_path).map_err(|err| {
        CommandError::new(
            "DISK_STATUS_READ_FAILED",
            "failed to read system disk free space.",
            trace_id.clone(),
            true,
            json!({ "mount_point": mount_point, "error": err.to_string() }),
        )
    })?;
    let total_bytes = total_space(root_path).map_err(|err| {
        CommandError::new(
            "DISK_STATUS_READ_FAILED",
            "failed to read system disk total space.",
            trace_id.clone(),
            true,
            json!({ "mount_point": mount_point, "error": err.to_string() }),
        )
    })?;

    Ok(DiskStatus {
        mount_point,
        display_name: "System Disk".to_string(),
        is_mounted,
        is_writable,
        free_bytes,
        total_bytes,
    })
}

#[tauri::command]
pub fn open_in_finder(path: String) -> Result<(), CommandError> {
    let trace_id = new_trace_id();
    let input = path.trim();
    if input.is_empty() {
        return Err(CommandError::new(
            "OPEN_PATH_INVALID",
            "path cannot be empty.",
            trace_id,
            false,
            json!({ "path": path }),
        ));
    }

    let resolved = expand_tilde(input);
    let resolved_path = PathBuf::from(&resolved);
    if !resolved_path.is_absolute() {
        return Err(CommandError::new(
            "OPEN_PATH_INVALID",
            "path must be absolute.",
            trace_id,
            false,
            json!({ "path": resolved }),
        ));
    }

    if !resolved_path.exists() {
        return Err(CommandError::new(
            "OPEN_PATH_NOT_FOUND",
            "path does not exist.",
            trace_id,
            false,
            json!({ "path": resolved }),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&resolved_path)
            .status()
            .map_err(|err| {
                CommandError::new(
                    "OPEN_PATH_FAILED",
                    "failed to open path in Finder.",
                    trace_id.clone(),
                    true,
                    json!({
                        "path": resolved,
                        "error": err.to_string()
                    }),
                )
            })?;

        if !status.success() {
            return Err(CommandError::new(
                "OPEN_PATH_FAILED",
                "failed to open path in Finder.",
                trace_id,
                true,
                json!({
                    "path": resolved,
                    "status": status.code()
                }),
            ));
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = resolved_path;
        return Err(CommandError::new(
            "OPEN_PATH_UNSUPPORTED",
            "open_in_finder is only supported on macOS.",
            trace_id,
            false,
            json!({}),
        ));
    }

    Ok(())
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

    if profile.availability == "blocked" || profile.availability == "deprecated" {
        return Err(CommandError::new(
            "PRECHECK_APP_BLOCKED",
            "Blocked or deprecated profile cannot be migrated.",
            trace_id,
            false,
            json!({
              "app_id": profile.app_id,
              "availability": profile.availability
            }),
        ));
    }

    let unit_hint = req.unit_id.as_deref();
    let selected_unit = pick_relocation_unit(&profile, unit_hint).ok_or_else(|| {
        if let Some(unit_id) = unit_hint {
            return CommandError::new(
                "PRECHECK_UNIT_NOT_FOUND",
                "No relocation unit found for app_id and unit_id.",
                trace_id.clone(),
                false,
                json!({ "app_id": profile.app_id, "unit_id": unit_id }),
            );
        }
        CommandError::new(
            "PRECHECK_SOURCE_NOT_FOUND",
            "No source path defined for profile.",
            trace_id.clone(),
            false,
            json!({ "app_id": profile.app_id }),
        )
    })?;
    let source_path = selected_unit.source_path.clone();
    let selected_unit_id = selected_unit.unit_id.clone();

    if !selected_unit.enabled || selected_unit.blocked_reason.is_some() {
        return Err(CommandError::new(
            "PRECHECK_UNIT_BLOCKED",
            "Selected relocation unit is disabled or blocked.",
            trace_id,
            false,
            json!({
              "app_id": profile.app_id,
              "unit_id": selected_unit_id,
              "blocked_reason": selected_unit.blocked_reason.clone()
            }),
        ));
    }

    if selected_unit.requires_confirmation && !req.confirm_high_risk {
        return Err(CommandError::new(
            "PRECHECK_UNIT_CONFIRMATION_REQUIRED",
            "Selected relocation unit requires explicit confirmation.",
            trace_id,
            true,
            json!({ "app_id": profile.app_id, "unit_id": selected_unit_id }),
        ));
    }

    if req.mode == "bootstrap" && !selected_unit.allow_bootstrap_if_source_missing {
        return Err(CommandError::new(
            "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
            "This relocation unit does not allow bootstrap mode.",
            trace_id,
            false,
            json!({ "app_id": profile.app_id, "unit_id": selected_unit_id }),
        ));
    }

    let relocation_id = new_relocation_id();
    let target_path = target_path_for_unit(&selected_unit, &req.target_root);
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
            selected_unit.allow_bootstrap_if_source_missing,
            profile.precheck_rules.require_full_disk_access,
            &trace_id,
        )?;
        Ok(json!({
          "source_path": source_path,
          "unit_id": selected_unit_id,
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
pub fn list_operation_logs(
    req: Option<OperationLogsRequest>,
    state: State<'_, AppState>,
) -> Result<Vec<OperationLogItem>, CommandError> {
    let trace_id = new_trace_id();
    let request = req.unwrap_or_default();
    let relocation_filter = request.relocation_id.clone();
    let trace_filter = request.trace_id.clone();
    let limit = request.limit.unwrap_or(400).clamp(1, 5000) as usize;

    let rows = state
        .db
        .list_operation_logs(relocation_filter.as_deref(), trace_filter.as_deref())
        .map_err(|err| db_error(&trace_id, "list operation logs", err))?;
    let mut items: Vec<OperationLogItem> = rows.into_iter().map(operation_log_to_item).collect();
    if items.len() > limit {
        let start = items.len() - limit;
        items = items.split_off(start);
    }
    Ok(items)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{PrecheckRules, RelocationUnit};
    use tempfile::tempdir;

    #[test]
    fn check_source_allows_missing_path_in_bootstrap_mode() {
        let dir = tempdir().expect("create temp dir");
        let missing = dir.path().join("missing-source");

        let size = check_source(
            "bootstrap",
            missing.to_string_lossy().as_ref(),
            true,
            false,
            "tr_test",
        )
        .expect("bootstrap should allow missing source path");

        assert_eq!(size, 0);
    }

    #[test]
    fn check_source_rejects_missing_path_in_migrate_mode() {
        let dir = tempdir().expect("create temp dir");
        let missing = dir.path().join("missing-source");

        let err = check_source(
            "migrate",
            missing.to_string_lossy().as_ref(),
            true,
            false,
            "tr_test",
        )
        .expect_err("migrate mode should reject missing source path");

        assert_eq!(err.code, "PRECHECK_SOURCE_NOT_FOUND");
    }

    #[test]
    fn check_source_rejects_file_source_path() {
        let dir = tempdir().expect("create temp dir");
        let file_path = dir.path().join("source-file");
        fs::write(&file_path, b"data").expect("write source file");

        let err = check_source(
            "migrate",
            file_path.to_string_lossy().as_ref(),
            false,
            false,
            "tr_test",
        )
        .expect_err("file source should be rejected");

        assert_eq!(err.code, "PRECHECK_SOURCE_NOT_FOUND");
    }

    #[test]
    fn check_source_bootstrap_mode_with_existing_dir_returns_size() {
        let dir = tempdir().expect("create temp dir");
        let source = dir.path().join("source");
        fs::create_dir_all(&source).expect("create source dir");
        fs::write(source.join("payload.txt"), b"hello").expect("write payload");

        let size = check_source(
            "bootstrap",
            source.to_string_lossy().as_ref(),
            true,
            false,
            "tr_test",
        )
        .expect("bootstrap source with data should return size");
        assert!(size > 0);
    }

    #[cfg(unix)]
    #[test]
    fn check_source_rejects_symlink_source_path() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().expect("create temp dir");
        let target = dir.path().join("target");
        let source = dir.path().join("source-link");
        fs::create_dir_all(&target).expect("create target dir");
        unix_fs::symlink(&target, &source).expect("create symlink source");

        let err = check_source(
            "migrate",
            source.to_string_lossy().as_ref(),
            false,
            false,
            "tr_test",
        )
        .expect_err("source symlink should be rejected");

        assert_eq!(err.code, "PRECHECK_SOURCE_IS_SYMLINK");
    }

    #[test]
    fn get_system_disk_status_returns_valid_root_metrics() {
        let status = get_system_disk_status().expect("read system disk status");
        assert_eq!(status.mount_point, "/");
        assert!(status.is_mounted);
        assert!(status.total_bytes > 0);
        assert!(status.total_bytes >= status.free_bytes);
    }

    #[test]
    fn profile_relocation_units_filters_empty_source_units() {
        let profile = AppProfile {
            app_id: "demo".to_string(),
            display_name: "Demo".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec![],
            relocation_units: vec![
                RelocationUnit {
                    unit_id: "u_media".to_string(),
                    display_name: "Media".to_string(),
                    source_path: "~/Library/Demo/Media".to_string(),
                    target_path_template: "{target_root}/AppData/Demo/Media".to_string(),
                    default_enabled: true,
                    enabled: true,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: None,
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
                RelocationUnit {
                    unit_id: "u_empty".to_string(),
                    display_name: "Empty".to_string(),
                    source_path: "   ".to_string(),
                    target_path_template: "{target_root}/AppData/Demo/Empty".to_string(),
                    default_enabled: true,
                    enabled: true,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: None,
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
            ],
            precheck_rules: PrecheckRules::default(),
        };

        let units = profile_relocation_units(&profile);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].unit_id, "u_media");
        assert!(units[0].source_path.ends_with("/Library/Demo/Media"));
    }

    #[test]
    fn pick_relocation_unit_prefers_existing_default_path() {
        let dir = tempdir().expect("create temp dir");
        let missing_path = dir.path().join("missing");
        let existing_path = dir.path().join("existing");
        fs::create_dir_all(&existing_path).expect("create existing source path");

        let profile = AppProfile {
            app_id: "demo".to_string(),
            display_name: "Demo".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec![],
            relocation_units: vec![
                RelocationUnit {
                    unit_id: "first".to_string(),
                    display_name: "First".to_string(),
                    source_path: missing_path.to_string_lossy().to_string(),
                    target_path_template: "{target_root}/AppData/Demo/First".to_string(),
                    default_enabled: true,
                    enabled: true,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: None,
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
                RelocationUnit {
                    unit_id: "second".to_string(),
                    display_name: "Second".to_string(),
                    source_path: existing_path.to_string_lossy().to_string(),
                    target_path_template: "{target_root}/AppData/Demo/Second".to_string(),
                    default_enabled: true,
                    enabled: true,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: None,
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
            ],
            precheck_rules: PrecheckRules::default(),
        };

        let selected = pick_relocation_unit(&profile, None).expect("select default unit");
        assert_eq!(selected.unit_id, "second");
    }

    #[test]
    fn pick_relocation_unit_honors_explicit_unit_id() {
        let profile = AppProfile {
            app_id: "demo".to_string(),
            display_name: "Demo".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec![],
            relocation_units: vec![
                RelocationUnit {
                    unit_id: "alpha".to_string(),
                    display_name: "Alpha".to_string(),
                    source_path: "/tmp/demo-alpha".to_string(),
                    target_path_template: "{target_root}/AppData/Demo/Alpha".to_string(),
                    default_enabled: false,
                    enabled: false,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: Some("disabled".to_string()),
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
                RelocationUnit {
                    unit_id: "beta".to_string(),
                    display_name: "Beta".to_string(),
                    source_path: "/tmp/demo-beta".to_string(),
                    target_path_template: "{target_root}/AppData/Demo/Beta".to_string(),
                    default_enabled: true,
                    enabled: true,
                    risk_level: "stable".to_string(),
                    requires_confirmation: false,
                    blocked_reason: None,
                    allow_bootstrap_if_source_missing: false,
                    category: "app-data".to_string(),
                },
            ],
            precheck_rules: PrecheckRules::default(),
        };

        let selected = pick_relocation_unit(&profile, Some("alpha")).expect("select alpha");
        assert_eq!(selected.unit_id, "alpha");
    }

    #[test]
    fn target_path_for_unit_uses_template_placeholder() {
        let unit = MigrationUnitPlan {
            unit_id: "main-data".to_string(),
            display_name: "Main Data".to_string(),
            source_path: "~/Library/WeChat".to_string(),
            target_path_template: "{target_root}/AppData/WeChat".to_string(),
            default_enabled: true,
            enabled: true,
            risk_level: "stable".to_string(),
            requires_confirmation: false,
            blocked_reason: None,
            allow_bootstrap_if_source_missing: false,
            category: "app-data".to_string(),
        };
        let target = target_path_for_unit(&unit, "/Volumes/TestSSD");
        assert_eq!(target, "/Volumes/TestSSD/AppData/WeChat");
    }

    #[test]
    fn target_path_for_unit_falls_back_to_default_layout_when_template_empty() {
        let unit = MigrationUnitPlan {
            unit_id: "custom".to_string(),
            display_name: "Custom App".to_string(),
            source_path: "/tmp/custom".to_string(),
            target_path_template: String::new(),
            default_enabled: true,
            enabled: true,
            risk_level: "stable".to_string(),
            requires_confirmation: false,
            blocked_reason: None,
            allow_bootstrap_if_source_missing: false,
            category: "app-data".to_string(),
        };
        let target = target_path_for_unit(&unit, "/Volumes/External");
        assert_eq!(target, "/Volumes/External/AppData/Custom App");
    }

    #[test]
    fn check_available_space_rejects_insufficient_capacity() {
        let dir = tempdir().expect("create temp dir");
        let root = dir.path().to_string_lossy().to_string();

        let err = check_available_space(&root, u64::MAX / 2, "tr_test")
            .expect_err("should reject when required space is impossible");
        assert_eq!(err.code, "PRECHECK_INSUFFICIENT_SPACE");
    }

    #[test]
    fn check_target_online_creates_missing_root_when_parent_exists() {
        let dir = tempdir().expect("create temp dir");
        let missing_root = dir.path().join("missing-root");
        check_target_online(missing_root.to_string_lossy().as_ref(), "tr_test")
            .expect("missing target root should be auto-created");
        assert!(missing_root.is_dir());
    }

    #[test]
    fn check_target_online_rejects_missing_parent() {
        let dir = tempdir().expect("create temp dir");
        let missing_root = dir.path().join("missing-parent").join("missing-root");
        let err = check_target_online(missing_root.to_string_lossy().as_ref(), "tr_test")
            .expect_err("missing parent should still be offline");
        assert_eq!(err.code, "PRECHECK_DISK_OFFLINE");
    }

    #[test]
    fn check_target_online_accepts_existing_directory() {
        let dir = tempdir().expect("create temp dir");
        check_target_online(dir.path().to_string_lossy().as_ref(), "tr_test")
            .expect("existing directory should be online");
    }

    #[test]
    fn check_target_writable_accepts_writable_root() {
        let dir = tempdir().expect("create temp dir");
        check_target_writable(dir.path().to_string_lossy().as_ref(), "tr_test")
            .expect("temp dir should be writable");
    }

    #[test]
    fn check_target_writable_rejects_missing_root() {
        let dir = tempdir().expect("create temp dir");
        let missing_root = dir.path().join("missing-root");
        let err = check_target_writable(missing_root.to_string_lossy().as_ref(), "tr_test")
            .expect_err("missing root should fail writable check");
        assert_eq!(err.code, "PRECHECK_DISK_OFFLINE");
    }

    #[cfg(unix)]
    #[test]
    fn postcheck_symlink_target_success() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().expect("create temp dir");
        let source = dir.path().join("source-link");
        let target = dir.path().join("target-dir");
        fs::create_dir_all(&target).expect("create target");
        unix_fs::symlink(&target, &source).expect("create source symlink");

        let result = postcheck_symlink_target(
            source.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
            "tr_test",
        )
        .expect("postcheck should pass");
        assert_eq!(result["symlink_ok"], true);
        assert_eq!(result["target_writable_ok"], true);
    }

    #[cfg(unix)]
    #[test]
    fn postcheck_symlink_target_rejects_non_symlink_source() {
        let dir = tempdir().expect("create temp dir");
        let source = dir.path().join("source-dir");
        let target = dir.path().join("target-dir");
        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&target).expect("create target");

        let err = postcheck_symlink_target(
            source.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
            "tr_test",
        )
        .expect_err("non-symlink source should fail");
        assert_eq!(err.code, "MIGRATE_POSTCHECK_FAILED");
    }

    #[cfg(unix)]
    #[test]
    fn postcheck_symlink_target_rejects_mismatched_target() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().expect("create temp dir");
        let source = dir.path().join("source-link");
        let target = dir.path().join("target-dir");
        let wrong_target = dir.path().join("wrong-target");
        fs::create_dir_all(&target).expect("create target");
        fs::create_dir_all(&wrong_target).expect("create wrong target");
        unix_fs::symlink(&wrong_target, &source).expect("create source symlink");

        let err = postcheck_symlink_target(
            source.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
            "tr_test",
        )
        .expect_err("mismatched target should fail");
        assert_eq!(err.code, "MIGRATE_POSTCHECK_FAILED");
    }

    #[test]
    fn operation_log_to_item_handles_invalid_details_json() {
        let row = OperationLogRecord {
            log_id: "log_test_invalid_json".to_string(),
            relocation_id: "reloc_test".to_string(),
            trace_id: "tr_test".to_string(),
            stage: "migration".to_string(),
            step: "copy_to_temp".to_string(),
            status: "failed".to_string(),
            error_code: Some("MIGRATE_COPY_FAILED".to_string()),
            duration_ms: Some(12),
            message: Some("copy failed".to_string()),
            details_json: "{invalid-json".to_string(),
            created_at: "2026-03-05T10:00:00Z".to_string(),
        };

        let item = operation_log_to_item(row);
        let parse_error = item
            .details
            .get("_parse_error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(parse_error, "invalid details_json payload");
    }

    #[test]
    fn health_event_to_model_uses_default_message_without_details_message() {
        let row = HealthEventRecord {
            snapshot_id: "snap_test".to_string(),
            relocation_id: "reloc_test".to_string(),
            app_id: "wechat-non-mas".to_string(),
            state: "healthy".to_string(),
            check_code: "HEALTH_OK".to_string(),
            details_json: "{}".to_string(),
            observed_at: "2026-03-05T10:00:00Z".to_string(),
        };

        let event = health_event_to_model(row);
        assert_eq!(event.message, "health event captured");
    }

    #[test]
    fn health_event_to_model_extracts_message_from_details() {
        let row = HealthEventRecord {
            snapshot_id: "snap_test_2".to_string(),
            relocation_id: "reloc_test".to_string(),
            app_id: "wechat-non-mas".to_string(),
            state: "degraded".to_string(),
            check_code: "HEALTH_DISK_OFFLINE".to_string(),
            details_json: r#"{ "message": "disk offline detected" }"#.to_string(),
            observed_at: "2026-03-05T10:00:00Z".to_string(),
        };

        let event = health_event_to_model(row);
        assert_eq!(event.message, "disk offline detected");
    }

    #[test]
    fn match_profile_bundle_prefers_bundle_id_when_available() {
        let profile = AppProfile {
            app_id: "wechat-non-mas".to_string(),
            display_name: "WeChat".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec!["com.tencent.xinWeChat".to_string()],
            process_names: vec!["WeChat".to_string()],
            relocation_units: vec![],
            precheck_rules: PrecheckRules::default(),
        };

        let bundles = vec![
            InstalledBundle {
                path: PathBuf::from("/Applications/WrongWeChat.app"),
                stem_lc: "wechat".to_string(),
                bundle_id_lc: Some("com.example.wrong".to_string()),
            },
            InstalledBundle {
                path: PathBuf::from("/Applications/WeChat.app"),
                stem_lc: "wechat".to_string(),
                bundle_id_lc: Some("com.tencent.xinwechat".to_string()),
            },
        ];

        let matched = match_profile_bundle(&profile, &bundles).expect("should match by bundle id");
        assert_eq!(matched, PathBuf::from("/Applications/WeChat.app"));
    }

    #[test]
    fn match_profile_bundle_falls_back_to_hint_matching() {
        let profile = AppProfile {
            app_id: "telegram-desktop".to_string(),
            display_name: "Telegram Desktop".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec!["Telegram".to_string()],
            relocation_units: vec![],
            precheck_rules: PrecheckRules::default(),
        };

        let bundles = vec![InstalledBundle {
            path: PathBuf::from("/Applications/Telegram.app"),
            stem_lc: "telegram".to_string(),
            bundle_id_lc: None,
        }];

        let matched = match_profile_bundle(&profile, &bundles).expect("should match by hint");
        assert_eq!(matched, PathBuf::from("/Applications/Telegram.app"));
    }

    #[test]
    fn profile_match_hints_include_jetbrains_specific_tokens() {
        let profile = AppProfile {
            app_id: "jetbrains-caches".to_string(),
            display_name: "JetBrains Caches".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec!["idea".to_string()],
            relocation_units: vec![],
            precheck_rules: PrecheckRules::default(),
        };

        let hints = profile_match_hints(&profile);
        assert!(hints.contains(&"android studio".to_string()));
        assert!(hints.contains(&"pycharm".to_string()));
    }

    #[test]
    fn profile_match_hints_include_display_name_app_id_and_bundle_id() {
        let profile = AppProfile {
            app_id: "wechat-non-mas".to_string(),
            display_name: "WeChat (Non-MAS)".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec!["com.tencent.xinWeChat".to_string()],
            process_names: vec!["WeChat".to_string()],
            relocation_units: vec![],
            precheck_rules: PrecheckRules::default(),
        };

        let hints = profile_match_hints(&profile);
        assert!(hints.contains(&"wechat (non-mas)".to_string()));
        assert!(hints.contains(&"wechat-non-mas".to_string()));
        assert!(hints.contains(&"com.tencent.xinwechat".to_string()));
    }

    #[test]
    fn resolve_profile_display_name_falls_back_without_bundle() {
        let profile = AppProfile {
            app_id: "demo-app".to_string(),
            display_name: "Demo App".to_string(),
            description_i18n: Default::default(),
            availability: "active".to_string(),
            blocked_reason: None,
            bundle_ids: vec![],
            process_names: vec![],
            relocation_units: vec![],
            precheck_rules: PrecheckRules::default(),
        };

        let name = resolve_profile_display_name(&profile, None);
        assert_eq!(name, "Demo App");
    }

    #[test]
    fn expand_tilde_keeps_plain_path_unchanged() {
        let input = "/tmp/demo";
        assert_eq!(expand_tilde(input), input);
    }

    #[test]
    fn mount_root_from_target_root_normalizes_volume_subpath() {
        assert_eq!(
            mount_root_from_target_root("/Volumes/M4_Ext_SSD/DataDock"),
            Some("/Volumes/M4_Ext_SSD".to_string())
        );
        assert_eq!(
            mount_root_from_target_root("/Volumes/M4_Ext_SSD"),
            Some("/Volumes/M4_Ext_SSD".to_string())
        );
        assert_eq!(mount_root_from_target_root("/Volumes"), None);
        assert_eq!(mount_root_from_target_root("/Volumes/"), None);
    }

    #[test]
    fn mount_root_from_target_root_trims_and_accepts_non_volume_path() {
        assert_eq!(mount_root_from_target_root("   "), None);
        assert_eq!(
            mount_root_from_target_root("/Users/cola/ExternalTarget"),
            Some("/Users/cola/ExternalTarget".to_string())
        );
        assert_eq!(mount_root_from_target_root("/"), Some("/".to_string()));
    }

    #[test]
    fn icon_mime_from_ext_maps_known_extensions_and_fallback() {
        assert_eq!(icon_mime_from_ext(Path::new("icon.jpg")), "image/jpeg");
        assert_eq!(icon_mime_from_ext(Path::new("icon.webp")), "image/webp");
        assert_eq!(icon_mime_from_ext(Path::new("icon.gif")), "image/gif");
        assert_eq!(icon_mime_from_ext(Path::new("icon.unknown")), "image/png");
    }

    #[test]
    fn read_icon_data_url_builds_data_url_for_png_file() {
        let dir = tempdir().expect("create temp dir");
        let icon_path = dir.path().join("icon.png");
        fs::write(&icon_path, b"PNG").expect("write icon file");

        let data_url =
            read_icon_data_url(icon_path.to_string_lossy().as_ref()).expect("read icon data url");
        assert!(data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn read_icon_data_url_returns_none_for_empty_file() {
        let dir = tempdir().expect("create temp dir");
        let icon_path = dir.path().join("empty.png");
        fs::write(&icon_path, b"").expect("write empty icon");

        let data_url = read_icon_data_url(icon_path.to_string_lossy().as_ref());
        assert!(data_url.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn directory_size_ignores_symlink_entries() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().expect("create temp dir");
        let target = dir.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        fs::write(target.join("payload.txt"), b"hello").expect("write payload");

        let link = dir.path().join("source-link");
        unix_fs::symlink(&target, &link).expect("create symlink");

        let size = directory_size(&link).expect("calculate size");
        assert_eq!(size, 0);
    }
}
