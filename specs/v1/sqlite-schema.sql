-- Disk Relocator SQLite Schema v1
-- Created at: 2026-03-05

PRAGMA foreign_keys = ON;

-- 迁移主记录
CREATE TABLE IF NOT EXISTS relocations (
  relocation_id TEXT PRIMARY KEY,
  app_id TEXT NOT NULL,
  tier TEXT NOT NULL CHECK (tier IN ('supported', 'experimental', 'blocked')),
  mode TEXT NOT NULL CHECK (mode IN ('bootstrap', 'migrate')),
  source_path TEXT NOT NULL,
  target_root TEXT NOT NULL,
  target_path TEXT NOT NULL,
  backup_path TEXT,
  state TEXT NOT NULL CHECK (
    state IN (
      'PRECHECKING',
      'PRECHECK_FAILED',
      'BOOTSTRAP_INIT',
      'COPYING',
      'VERIFYING',
      'SWITCHING',
      'POSTCHECKING',
      'HEALTHY',
      'DEGRADED',
      'BROKEN',
      'FAILED_NEEDS_ROLLBACK',
      'ROLLING_BACK',
      'ROLLED_BACK',
      'ROLLBACK_FAILED'
    )
  ),
  health_state TEXT NOT NULL CHECK (health_state IN ('healthy', 'degraded', 'broken', 'unknown')),
  last_error_code TEXT,
  trace_id TEXT NOT NULL,
  source_size_bytes INTEGER NOT NULL DEFAULT 0,
  target_size_bytes INTEGER NOT NULL DEFAULT 0,
  metadata_version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_relocations_app_id ON relocations (app_id);
CREATE INDEX IF NOT EXISTS idx_relocations_state ON relocations (state);
CREATE INDEX IF NOT EXISTS idx_relocations_health_state ON relocations (health_state);
CREATE INDEX IF NOT EXISTS idx_relocations_trace_id ON relocations (trace_id);

-- 步骤级操作日志
CREATE TABLE IF NOT EXISTS operation_logs (
  log_id TEXT PRIMARY KEY,
  relocation_id TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  stage TEXT NOT NULL CHECK (stage IN ('precheck', 'migration', 'rollback', 'health')),
  step TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('started', 'succeeded', 'failed', 'skipped')),
  error_code TEXT,
  duration_ms INTEGER,
  message TEXT,
  details_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (relocation_id) REFERENCES relocations(relocation_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_operation_logs_relocation_id ON operation_logs (relocation_id);
CREATE INDEX IF NOT EXISTS idx_operation_logs_trace_id ON operation_logs (trace_id);
CREATE INDEX IF NOT EXISTS idx_operation_logs_stage ON operation_logs (stage);
CREATE INDEX IF NOT EXISTS idx_operation_logs_created_at ON operation_logs (created_at);

-- 健康快照
CREATE TABLE IF NOT EXISTS health_snapshots (
  snapshot_id TEXT PRIMARY KEY,
  relocation_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('healthy', 'degraded', 'broken')),
  check_code TEXT NOT NULL,
  details_json TEXT NOT NULL DEFAULT '{}',
  observed_at TEXT NOT NULL,
  FOREIGN KEY (relocation_id) REFERENCES relocations(relocation_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_health_snapshots_relocation_id ON health_snapshots (relocation_id);
CREATE INDEX IF NOT EXISTS idx_health_snapshots_state ON health_snapshots (state);
CREATE INDEX IF NOT EXISTS idx_health_snapshots_observed_at ON health_snapshots (observed_at);

-- 运行中的任务恢复视图
CREATE VIEW IF NOT EXISTS v_unfinished_relocations AS
SELECT relocation_id, app_id, state, trace_id, updated_at
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
);

