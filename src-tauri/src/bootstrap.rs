use serde_json::{json, Value};
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[derive(Debug, Clone, Default)]
pub struct BootstrapSwitchOutcome {
    pub source_placeholder_removed: bool,
    pub target_dir_created: bool,
}

#[derive(Debug, Clone)]
pub struct BootstrapSwitchError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
}

fn error(
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
    details: Value,
) -> BootstrapSwitchError {
    BootstrapSwitchError {
        code,
        message: message.into(),
        retryable,
        details,
    }
}

fn is_directory_empty(path: &Path) -> std::io::Result<bool> {
    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().is_none())
}

pub fn execute_bootstrap_switch(
    source_path: &Path,
    target_path: &Path,
) -> Result<BootstrapSwitchOutcome, BootstrapSwitchError> {
    let mut outcome = BootstrapSwitchOutcome::default();

    match fs::symlink_metadata(source_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(error(
                    "PRECHECK_SOURCE_IS_SYMLINK",
                    "source path is already a symlink.",
                    false,
                    json!({ "source_path": source_path }),
                ));
            }

            if metadata.is_dir() {
                let is_empty = is_directory_empty(source_path).map_err(|err| {
                    error(
                        "PRECHECK_SOURCE_NOT_FOUND",
                        "failed to inspect source directory.",
                        true,
                        json!({ "source_path": source_path, "error": err.to_string() }),
                    )
                })?;
                if !is_empty {
                    return Err(error(
                        "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
                        "bootstrap mode requires source path without existing data.",
                        false,
                        json!({ "source_path": source_path }),
                    ));
                }

                fs::remove_dir(source_path).map_err(|err| {
                    error(
                        "MIGRATE_SWITCH_RENAME_FAILED",
                        "failed to remove source placeholder directory.",
                        true,
                        json!({ "source_path": source_path, "error": err.to_string() }),
                    )
                })?;
                outcome.source_placeholder_removed = true;
            } else if metadata.is_file() {
                if metadata.len() > 0 {
                    return Err(error(
                        "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
                        "bootstrap mode requires source path without existing data.",
                        false,
                        json!({ "source_path": source_path }),
                    ));
                }
                fs::remove_file(source_path).map_err(|err| {
                    error(
                        "MIGRATE_SWITCH_RENAME_FAILED",
                        "failed to remove empty source placeholder file.",
                        true,
                        json!({ "source_path": source_path, "error": err.to_string() }),
                    )
                })?;
                outcome.source_placeholder_removed = true;
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(error(
                "PRECHECK_SOURCE_NOT_FOUND",
                "failed to inspect source path.",
                true,
                json!({ "source_path": source_path, "error": err.to_string() }),
            ))
        }
    }

    match fs::symlink_metadata(target_path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(error(
                    "MIGRATE_SWITCH_SYMLINK_FAILED",
                    "target path exists but is not a directory.",
                    false,
                    json!({ "target_path": target_path }),
                ));
            }

            let is_empty = is_directory_empty(target_path).map_err(|err| {
                error(
                    "PRECHECK_DISK_READONLY",
                    "failed to inspect target directory.",
                    true,
                    json!({ "target_path": target_path, "error": err.to_string() }),
                )
            })?;
            if !is_empty {
                return Err(error(
                    "PRECHECK_BOOTSTRAP_NOT_ALLOWED",
                    "target path already contains data.",
                    false,
                    json!({ "target_path": target_path }),
                ));
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(target_path).map_err(|create_err| {
                error(
                    "PRECHECK_DISK_READONLY",
                    "failed to create target directory for bootstrap mode.",
                    true,
                    json!({
                        "target_path": target_path,
                        "error": create_err.to_string()
                    }),
                )
            })?;
            outcome.target_dir_created = true;
        }
        Err(err) => {
            return Err(error(
                "PRECHECK_DISK_OFFLINE",
                "failed to inspect target path.",
                true,
                json!({ "target_path": target_path, "error": err.to_string() }),
            ))
        }
    }

    #[cfg(unix)]
    {
        symlink(target_path, source_path).map_err(|err| {
            let _ = rollback_bootstrap_switch(source_path, target_path, &outcome);
            error(
                "MIGRATE_SWITCH_SYMLINK_FAILED",
                "failed to create source -> target symlink.",
                true,
                json!({
                    "source_path": source_path,
                    "target_path": target_path,
                    "error": err.to_string()
                }),
            )
        })?;
    }

    Ok(outcome)
}

pub fn rollback_bootstrap_switch(
    source_path: &Path,
    target_path: &Path,
    outcome: &BootstrapSwitchOutcome,
) -> Result<(), BootstrapSwitchError> {
    if let Ok(metadata) = fs::symlink_metadata(source_path) {
        if metadata.file_type().is_symlink() {
            fs::remove_file(source_path).map_err(|err| {
                error(
                    "ROLLBACK_REMOVE_SYMLINK_FAILED",
                    "failed to remove bootstrap symlink during rollback.",
                    true,
                    json!({ "source_path": source_path, "error": err.to_string() }),
                )
            })?;
        }
    }

    if outcome.source_placeholder_removed && fs::symlink_metadata(source_path).is_err() {
        fs::create_dir_all(source_path).map_err(|err| {
            error(
                "ROLLBACK_RESTORE_BACKUP_FAILED",
                "failed to recreate source placeholder during rollback.",
                false,
                json!({ "source_path": source_path, "error": err.to_string() }),
            )
        })?;
    }

    if outcome.target_dir_created {
        if let Ok(metadata) = fs::symlink_metadata(target_path) {
            if metadata.is_dir() {
                let is_empty = is_directory_empty(target_path).map_err(|err| {
                    error(
                        "ROLLBACK_CLEANUP_TEMP_FAILED",
                        "failed to inspect target directory during rollback cleanup.",
                        true,
                        json!({ "target_path": target_path, "error": err.to_string() }),
                    )
                })?;
                if is_empty {
                    fs::remove_dir(target_path).map_err(|err| {
                        error(
                            "ROLLBACK_CLEANUP_TEMP_FAILED",
                            "failed to cleanup target directory during rollback.",
                            true,
                            json!({ "target_path": target_path, "error": err.to_string() }),
                        )
                    })?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bootstrap_creates_target_and_symlink_when_source_missing() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");

        let outcome =
            execute_bootstrap_switch(&source_path, &target_path).expect("bootstrap switch");
        assert!(!outcome.source_placeholder_removed);
        assert!(outcome.target_dir_created);
        assert!(target_path.is_dir());

        let source_metadata = fs::symlink_metadata(&source_path).expect("source metadata");
        assert!(source_metadata.file_type().is_symlink());
    }

    #[test]
    fn bootstrap_replaces_empty_source_placeholder() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&source_path).expect("create source dir");

        let outcome =
            execute_bootstrap_switch(&source_path, &target_path).expect("bootstrap switch");
        assert!(outcome.source_placeholder_removed);
        assert!(outcome.target_dir_created);

        let source_metadata = fs::symlink_metadata(&source_path).expect("source metadata");
        assert!(source_metadata.file_type().is_symlink());
    }

    #[test]
    fn bootstrap_fails_when_source_has_existing_data() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        fs::create_dir_all(&source_path).expect("create source dir");
        fs::write(source_path.join("data.txt"), b"hello").expect("write payload");
        let target_path = dir.path().join("target");

        let err = execute_bootstrap_switch(&source_path, &target_path).expect_err("expect failure");
        assert_eq!(err.code, "PRECHECK_BOOTSTRAP_NOT_ALLOWED");
        assert!(source_path.is_dir());
        assert!(!target_path.exists());
    }

    #[test]
    fn rollback_restores_source_placeholder_and_cleans_target() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&source_path).expect("create source placeholder");

        let outcome =
            execute_bootstrap_switch(&source_path, &target_path).expect("bootstrap switch");
        rollback_bootstrap_switch(&source_path, &target_path, &outcome).expect("rollback");

        assert!(source_path.is_dir());
        assert!(!target_path.exists());
    }

    #[test]
    fn bootstrap_fails_when_target_contains_existing_data() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&source_path).expect("create source placeholder");
        fs::create_dir_all(&target_path).expect("create target");
        fs::write(target_path.join("payload.txt"), b"hello").expect("write target payload");

        let err = execute_bootstrap_switch(&source_path, &target_path).expect_err("expect failure");
        assert_eq!(err.code, "PRECHECK_BOOTSTRAP_NOT_ALLOWED");
    }

    #[test]
    fn bootstrap_fails_when_target_exists_as_file() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target.file");
        fs::create_dir_all(&source_path).expect("create source placeholder");
        fs::write(&target_path, b"not a directory").expect("create target file");

        let err = execute_bootstrap_switch(&source_path, &target_path).expect_err("expect failure");
        assert_eq!(err.code, "MIGRATE_SWITCH_SYMLINK_FAILED");
    }

    #[test]
    fn rollback_does_not_remove_non_empty_target_dir() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&source_path).expect("create source placeholder");

        let outcome =
            execute_bootstrap_switch(&source_path, &target_path).expect("bootstrap switch");
        fs::write(target_path.join("payload.txt"), b"kept").expect("write target payload");

        rollback_bootstrap_switch(&source_path, &target_path, &outcome).expect("rollback");
        assert!(source_path.is_dir());
        assert!(target_path.is_dir());
        assert!(target_path.join("payload.txt").exists());
    }
}
