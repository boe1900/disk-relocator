# Acceptance Skeleton

This folder only contains skeleton scripts and scenario definitions.
No migration business logic is implemented here.

## Target scenarios

1. Bootstrap mode (first write directly to external disk)
2. Existing data migration mode
3. Rollback after failure
4. Health monitoring (online/offline/read-only)

## How to run

```bash
bash tests/acceptance/run-smoke.sh
bash tests/release/run-release-gate.sh
```

## Expected status (current phase)

- Contract-level checks pass (`npm build`, `cargo check`, profile JSON valid).
- Bootstrap scenario is implemented for real symlink switch and cleanup rollback.
- Migrate scenario is implemented for copy/verify/switch/postcheck with auto rollback.
- Manual rollback is implemented for source restoration and temp cleanup.
- Startup interruption recovery is implemented for unfinished relocation reconciliation.
- Health-monitor scenario is implemented with polling and mount-event trigger.
- Reconcile scenario is implemented for drift detection and safe-fix.
