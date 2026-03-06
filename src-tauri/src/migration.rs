use serde_json::{json, Value};
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[derive(Debug, Clone)]
pub struct MigrationError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub struct CopyResult {
    pub copied_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub source_size_bytes: u64,
    pub temp_size_bytes: u64,
}

fn error(
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
    details: Value,
) -> MigrationError {
    MigrationError {
        code,
        message: message.into(),
        retryable,
        details,
    }
}

fn path_as_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        Ok(())
    }
}

fn copy_recursive(source: &Path, target: &Path) -> std::io::Result<u64> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("symlink entry is not supported: {}", path_as_string(source)),
        ));
    }
    if metadata.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    fs::create_dir_all(target)?;
    let mut total = 0_u64;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_child = entry.path();
        let target_child = target.join(entry.file_name());
        total = total.saturating_add(copy_recursive(&source_child, &target_child)?);
    }
    Ok(total)
}

fn size_recursive(path: &Path) -> std::io::Result<u64> {
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
        total = total.saturating_add(size_recursive(&entry.path())?);
    }
    Ok(total)
}

pub fn cleanup_temp_path(temp_path: &Path) -> Result<(), MigrationError> {
    if !temp_path.exists() {
        return Ok(());
    }

    remove_path(temp_path).map_err(|err| {
        error(
            "ROLLBACK_CLEANUP_TEMP_FAILED",
            "failed to cleanup migration temp path.",
            true,
            json!({ "temp_path": temp_path, "error": err.to_string() }),
        )
    })
}

pub fn remove_path_if_exists(path: &Path) -> Result<(), MigrationError> {
    if !path.exists() {
        return Ok(());
    }
    remove_path(path).map_err(|err| {
        error(
            "ROLLBACK_CLEANUP_TEMP_FAILED",
            "failed to remove existing path.",
            true,
            json!({ "path": path, "error": err.to_string() }),
        )
    })
}

pub fn copy_path_to_path(source_path: &Path, target_path: &Path) -> Result<u64, MigrationError> {
    if !source_path.exists() {
        return Err(error(
            "ROLLBACK_RESTORE_BACKUP_FAILED",
            "source path does not exist for rollback copy.",
            false,
            json!({ "source_path": source_path }),
        ));
    }
    if target_path.exists() {
        return Err(error(
            "ROLLBACK_RESTORE_BACKUP_FAILED",
            "target path already exists for rollback copy.",
            false,
            json!({ "target_path": target_path }),
        ));
    }

    copy_recursive(source_path, target_path).map_err(|err| {
        error(
            "ROLLBACK_RESTORE_BACKUP_FAILED",
            "failed to copy target data back to source path during rollback.",
            true,
            json!({
                "source_path": source_path,
                "target_path": target_path,
                "error": err.to_string()
            }),
        )
    })
}

pub fn copy_source_to_temp(
    source_path: &Path,
    temp_path: &Path,
) -> Result<CopyResult, MigrationError> {
    if !source_path.exists() {
        return Err(error(
            "PRECHECK_SOURCE_NOT_FOUND",
            "source path does not exist.",
            false,
            json!({ "source_path": source_path }),
        ));
    }

    if temp_path.exists() {
        cleanup_temp_path(temp_path)?;
    }

    let copied_bytes = copy_recursive(source_path, temp_path).map_err(|err| {
        error(
            "MIGRATE_COPY_FAILED",
            "failed during copy source -> temp.",
            true,
            json!({
                "source_path": source_path,
                "temp_path": temp_path,
                "error": err.to_string()
            }),
        )
    })?;

    Ok(CopyResult { copied_bytes })
}

pub fn verify_source_and_temp(
    source_path: &Path,
    temp_path: &Path,
) -> Result<VerifyResult, MigrationError> {
    if !temp_path.exists() {
        return Err(error(
            "MIGRATE_VERIFY_SIZE_MISMATCH",
            "temp path does not exist for verification.",
            false,
            json!({ "temp_path": temp_path }),
        ));
    }

    let source_size_bytes = size_recursive(source_path).map_err(|err| {
        error(
            "MIGRATE_VERIFY_SIZE_MISMATCH",
            "failed to calculate source size.",
            true,
            json!({ "source_path": source_path, "error": err.to_string() }),
        )
    })?;
    let temp_size_bytes = size_recursive(temp_path).map_err(|err| {
        error(
            "MIGRATE_VERIFY_SIZE_MISMATCH",
            "failed to calculate temp size.",
            true,
            json!({ "temp_path": temp_path, "error": err.to_string() }),
        )
    })?;

    if source_size_bytes != temp_size_bytes {
        return Err(error(
            "MIGRATE_VERIFY_SIZE_MISMATCH",
            "source and temp size mismatch.",
            false,
            json!({
                "source_path": source_path,
                "temp_path": temp_path,
                "source_size_bytes": source_size_bytes,
                "temp_size_bytes": temp_size_bytes
            }),
        ));
    }

    Ok(VerifyResult {
        source_size_bytes,
        temp_size_bytes,
    })
}

pub fn switch_to_symlink(
    source_path: &Path,
    temp_path: &Path,
    target_path: &Path,
    backup_path: &Path,
) -> Result<(), MigrationError> {
    if backup_path.exists() {
        return Err(error(
            "PRECHECK_BACKUP_PATH_EXISTS",
            "backup path already exists.",
            false,
            json!({ "backup_path": backup_path }),
        ));
    }

    if target_path.exists() {
        return Err(error(
            "PRECHECK_TARGET_PATH_EXISTS",
            "target path already exists.",
            false,
            json!({ "target_path": target_path }),
        ));
    }

    if !temp_path.exists() {
        return Err(error(
            "MIGRATE_SWITCH_RENAME_FAILED",
            "temp path does not exist for switch stage.",
            false,
            json!({ "temp_path": temp_path }),
        ));
    }

    fs::rename(source_path, backup_path).map_err(|err| {
        error(
            "MIGRATE_SWITCH_RENAME_FAILED",
            "failed to rename source path to backup path.",
            true,
            json!({
                "source_path": source_path,
                "backup_path": backup_path,
                "error": err.to_string()
            }),
        )
    })?;

    if let Err(rename_target_err) = fs::rename(temp_path, target_path) {
        let _ = fs::rename(backup_path, source_path);
        return Err(error(
            "MIGRATE_SWITCH_RENAME_FAILED",
            "failed to promote temp path to target path.",
            true,
            json!({
                "temp_path": temp_path,
                "target_path": target_path,
                "error": rename_target_err.to_string()
            }),
        ));
    }

    #[cfg(unix)]
    {
        if let Err(symlink_err) = symlink(target_path, source_path) {
            let _ = rollback_migration_paths(source_path, temp_path, target_path, backup_path);
            return Err(error(
                "MIGRATE_SWITCH_SYMLINK_FAILED",
                "failed to create source -> target symlink.",
                true,
                json!({
                    "source_path": source_path,
                    "target_path": target_path,
                    "error": symlink_err.to_string()
                }),
            ));
        }
    }

    Ok(())
}

pub fn rollback_migration_paths(
    source_path: &Path,
    temp_path: &Path,
    target_path: &Path,
    backup_path: &Path,
) -> Result<(), MigrationError> {
    if let Ok(metadata) = fs::symlink_metadata(source_path) {
        if metadata.file_type().is_symlink() {
            fs::remove_file(source_path).map_err(|err| {
                error(
                    "ROLLBACK_REMOVE_SYMLINK_FAILED",
                    "failed to remove source symlink during rollback.",
                    true,
                    json!({ "source_path": source_path, "error": err.to_string() }),
                )
            })?;
        }
    }

    if backup_path.exists() && !source_path.exists() {
        fs::rename(backup_path, source_path).map_err(|err| {
            error(
                "ROLLBACK_RESTORE_BACKUP_FAILED",
                "failed to restore source path from backup.",
                false,
                json!({
                    "backup_path": backup_path,
                    "source_path": source_path,
                    "error": err.to_string()
                }),
            )
        })?;
    }

    if temp_path.exists() {
        cleanup_temp_path(temp_path)?;
    }

    if target_path.exists() {
        remove_path(target_path).map_err(|err| {
            error(
                "ROLLBACK_CLEANUP_TEMP_FAILED",
                "failed to cleanup target path during rollback.",
                true,
                json!({ "target_path": target_path, "error": err.to_string() }),
            )
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn copy_verify_and_switch_success() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        fs::create_dir_all(&source_path).expect("create source");
        fs::write(source_path.join("a.txt"), b"hello").expect("write file");
        fs::create_dir_all(source_path.join("child")).expect("create child");
        fs::write(source_path.join("child").join("b.txt"), b"world").expect("write file");

        let temp_path = dir.path().join("target.tmp");
        let target_path = dir.path().join("target");
        let backup_path = dir.path().join("source.bak");

        let copy_result = copy_source_to_temp(&source_path, &temp_path).expect("copy");
        assert!(copy_result.copied_bytes > 0);

        let verify = verify_source_and_temp(&source_path, &temp_path).expect("verify");
        assert_eq!(verify.source_size_bytes, verify.temp_size_bytes);

        switch_to_symlink(&source_path, &temp_path, &target_path, &backup_path).expect("switch");
        let source_meta = fs::symlink_metadata(&source_path).expect("source meta");
        assert!(source_meta.file_type().is_symlink());
        assert!(target_path.is_dir());
        assert!(backup_path.is_dir());
    }

    #[test]
    fn rollback_restores_source_and_removes_target() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        fs::create_dir_all(&source_path).expect("create source");
        fs::write(source_path.join("a.txt"), b"hello").expect("write file");

        let temp_path = dir.path().join("target.tmp");
        let target_path = dir.path().join("target");
        let backup_path = dir.path().join("source.bak");

        copy_source_to_temp(&source_path, &temp_path).expect("copy");
        verify_source_and_temp(&source_path, &temp_path).expect("verify");
        switch_to_symlink(&source_path, &temp_path, &target_path, &backup_path).expect("switch");

        rollback_migration_paths(&source_path, &temp_path, &target_path, &backup_path)
            .expect("rollback");
        assert!(source_path.is_dir());
        assert!(!backup_path.exists());
        assert!(!target_path.exists());
        assert!(!temp_path.exists());
    }

    #[test]
    fn verify_detects_mismatch() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        fs::create_dir_all(&source_path).expect("create source");
        fs::write(source_path.join("a.txt"), b"hello").expect("write file");

        let temp_path = dir.path().join("target.tmp");
        copy_source_to_temp(&source_path, &temp_path).expect("copy");
        fs::write(temp_path.join("x.txt"), b"extra").expect("write extra");

        let err = verify_source_and_temp(&source_path, &temp_path).expect_err("expect mismatch");
        assert_eq!(err.code, "MIGRATE_VERIFY_SIZE_MISMATCH");
    }

    #[test]
    fn migration_and_rollback_20_rounds() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        fs::create_dir_all(&source_path).expect("create source");

        let target_path = dir.path().join("target");
        let backup_path = dir.path().join("source.bak");

        for round in 1..=20 {
            let round_file = source_path.join(format!("round-{round}.txt"));
            fs::write(&round_file, format!("payload-{round}")).expect("write round payload");

            let temp_path = dir.path().join(format!("target.tmp.round-{round}"));

            let copy_result = copy_source_to_temp(&source_path, &temp_path).expect("copy");
            assert!(copy_result.copied_bytes > 0);

            let verify = verify_source_and_temp(&source_path, &temp_path).expect("verify");
            assert_eq!(verify.source_size_bytes, verify.temp_size_bytes);

            switch_to_symlink(&source_path, &temp_path, &target_path, &backup_path)
                .expect("switch");
            let source_meta = fs::symlink_metadata(&source_path).expect("source meta");
            assert!(source_meta.file_type().is_symlink());
            assert!(target_path.exists());
            assert!(backup_path.exists());
            assert!(target_path.join(format!("round-{round}.txt")).exists());

            rollback_migration_paths(&source_path, &temp_path, &target_path, &backup_path)
                .expect("rollback");
            let source_meta_after = fs::symlink_metadata(&source_path).expect("source meta after");
            assert!(source_meta_after.is_dir());
            assert!(!source_meta_after.file_type().is_symlink());
            assert!(!target_path.exists());
            assert!(!backup_path.exists());
            assert!(!temp_path.exists());
            assert!(source_path.join(format!("round-{round}.txt")).exists());
        }
    }

    #[test]
    fn copy_source_to_temp_rejects_missing_source() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("missing-source");
        let temp_path = dir.path().join("target.tmp");

        let err = copy_source_to_temp(&source_path, &temp_path).expect_err("expect missing source");
        assert_eq!(err.code, "PRECHECK_SOURCE_NOT_FOUND");
    }

    #[test]
    fn switch_to_symlink_rejects_existing_target_path() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let temp_path = dir.path().join("target.tmp");
        let target_path = dir.path().join("target");
        let backup_path = dir.path().join("source.bak");

        fs::create_dir_all(&source_path).expect("create source");
        fs::write(source_path.join("a.txt"), b"hello").expect("write source file");
        copy_source_to_temp(&source_path, &temp_path).expect("copy to temp");
        fs::create_dir_all(&target_path).expect("create existing target");

        let err = switch_to_symlink(&source_path, &temp_path, &target_path, &backup_path)
            .expect_err("expect target exists failure");
        assert_eq!(err.code, "PRECHECK_TARGET_PATH_EXISTS");
    }

    #[test]
    fn copy_path_to_path_rejects_existing_target() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&source_path).expect("create source");
        fs::write(source_path.join("a.txt"), b"hello").expect("write source file");
        fs::create_dir_all(&target_path).expect("create target");

        let err = copy_path_to_path(&source_path, &target_path)
            .expect_err("expect target exists failure");
        assert_eq!(err.code, "ROLLBACK_RESTORE_BACKUP_FAILED");
    }

    #[test]
    fn rollback_removes_source_symlink_and_target_without_backup() {
        let dir = tempdir().expect("create tempdir");
        let source_path = dir.path().join("source");
        let temp_path = dir.path().join("target.tmp");
        let target_path = dir.path().join("target");
        let backup_path = dir.path().join("source.bak");

        fs::create_dir_all(&target_path).expect("create target");
        fs::write(target_path.join("data.txt"), b"payload").expect("write target file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_path, &source_path).expect("create source symlink");

        rollback_migration_paths(&source_path, &temp_path, &target_path, &backup_path)
            .expect("rollback cleanup");
        assert!(!source_path.exists());
        assert!(!target_path.exists());
        assert!(!backup_path.exists());
    }
}
