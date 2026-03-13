use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub trace_id: String,
    pub retryable: bool,
    pub details: Value,
}

impl CommandError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        trace_id: impl Into<String>,
        retryable: bool,
        details: Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            trace_id: trace_id.into(),
            retryable,
            details,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct MigrateRequest {
    pub app_id: String,
    #[serde(default)]
    pub unit_id: Option<String>,
    pub target_root: String,
    pub mode: String,
    #[serde(default)]
    pub trace_id: Option<String>,
    pub confirm_high_risk: bool,
    #[serde(default)]
    pub cleanup_backup_after_migrate: bool,
}

#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub relocation_id: String,
    pub force: bool,
    #[serde(default)]
    pub trace_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OperationLogsRequest {
    pub relocation_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct HealthEventsRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileRequest {
    pub apply_safe_fixes: Option<bool>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AppScanPath {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_bootstrap_if_source_missing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub path: String,
    pub exists: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct AppScanResult {
    pub app_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub description_i18n: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_data_url: Option<String>,
    pub availability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    pub detected_paths: Vec<AppScanPath>,
    pub running: bool,
    pub allow_bootstrap_if_source_missing: bool,
    pub last_verified_at: String,
}

#[derive(Debug, Serialize)]
pub struct DiskStatus {
    pub mount_point: String,
    pub display_name: String,
    pub is_mounted: bool,
    pub is_writable: bool,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct RelocationResult {
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub health_state: String,
    pub source_path: String,
    pub target_path: String,
    pub backup_path: Option<String>,
    pub trace_id: String,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct RelocationSummary {
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub health_state: String,
    pub source_path: String,
    pub target_path: String,
    pub source_size_bytes: i64,
    pub target_size_bytes: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct HealthCheck {
    pub code: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub checks: Vec<HealthCheck>,
    pub observed_at: String,
}

#[derive(Debug, Serialize)]
pub struct HealthEvent {
    pub snapshot_id: String,
    pub relocation_id: String,
    pub app_id: String,
    pub state: String,
    pub check_code: String,
    pub message: String,
    pub observed_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReconcileIssue {
    pub relocation_id: String,
    pub app_id: String,
    pub code: String,
    pub severity: String,
    pub message: String,
    pub suggestion: String,
    pub safe_fix_action: Option<String>,
    pub safe_fix_applied: bool,
    pub details: Value,
}

#[derive(Debug, Serialize)]
pub struct ReconcileResult {
    pub trace_id: String,
    pub observed_at: String,
    pub scanned: usize,
    pub drift_count: usize,
    pub safe_fixable_count: usize,
    pub fixed_count: usize,
    pub issues: Vec<ReconcileIssue>,
}

#[derive(Debug, Serialize)]
pub struct OperationLogItem {
    pub log_id: String,
    pub relocation_id: String,
    pub trace_id: String,
    pub stage: String,
    pub step: String,
    pub status: String,
    pub error_code: Option<String>,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
    pub details: Value,
    pub created_at: String,
}
