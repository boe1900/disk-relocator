export interface CommandError {
  code: string;
  message: string;
  trace_id: string;
  retryable: boolean;
  details: Record<string, unknown>;
}

export interface AppScanPath {
  path: string;
  exists: boolean;
  is_symlink: boolean;
  size_bytes: number;
}

export interface AppScanResult {
  app_id: string;
  display_name: string;
  tier: "supported" | "experimental" | "blocked";
  detected_paths: AppScanPath[];
  running: boolean;
  allow_bootstrap_if_source_missing: boolean;
  last_verified_at: string;
}

export interface DiskStatus {
  mount_point: string;
  display_name: string;
  is_mounted: boolean;
  is_writable: boolean;
  free_bytes: number;
  total_bytes: number;
}

export interface MigrateRequest {
  app_id: string;
  target_root: string;
  mode: "bootstrap" | "migrate";
  allow_experimental: boolean;
  cleanup_backup_after_migrate?: boolean;
}

export interface RollbackRequest {
  relocation_id: string;
  force: boolean;
}

export interface ExportLogsRequest {
  relocation_id?: string;
  trace_id?: string;
  output_path?: string;
}

export interface HealthEventsRequest {
  limit?: number;
}

export interface ReconcileRequest {
  apply_safe_fixes?: boolean;
  limit?: number;
}

export interface RelocationResult {
  relocation_id: string;
  app_id: string;
  state: string;
  health_state: string;
  source_path: string;
  target_path: string;
  backup_path: string | null;
  trace_id: string;
  started_at: string;
  updated_at: string;
}

export interface RelocationSummary {
  relocation_id: string;
  app_id: string;
  state: string;
  health_state: string;
  source_path: string;
  target_path: string;
  updated_at: string;
}

export interface HealthCheck {
  code: string;
  ok: boolean;
  message: string;
}

export interface HealthStatus {
  relocation_id: string;
  app_id: string;
  state: "healthy" | "degraded" | "broken";
  checks: HealthCheck[];
  observed_at: string;
}

export interface HealthEvent {
  snapshot_id: string;
  relocation_id: string;
  app_id: string;
  state: "healthy" | "degraded" | "broken";
  check_code: string;
  message: string;
  observed_at: string;
}

export interface ReconcileIssue {
  relocation_id: string;
  app_id: string;
  code: string;
  severity: "warning" | "critical";
  message: string;
  suggestion: string;
  safe_fix_action: string | null;
  safe_fix_applied: boolean;
  details: Record<string, unknown>;
}

export interface ReconcileResult {
  trace_id: string;
  observed_at: string;
  scanned: number;
  drift_count: number;
  safe_fixable_count: number;
  fixed_count: number;
  issues: ReconcileIssue[];
}

export interface OperationLogItem {
  log_id: string;
  relocation_id: string;
  trace_id: string;
  stage: "precheck" | "migration" | "rollback" | "health";
  step: string;
  status: "started" | "succeeded" | "failed" | "skipped";
  error_code: string | null;
  duration_ms: number | null;
  message: string | null;
  details: Record<string, unknown>;
  created_at: string;
}

export interface ExportLogsResult {
  export_trace_id: string;
  relocation_id: string | null;
  trace_id: string | null;
  output_path: string;
  exported_count: number;
}
