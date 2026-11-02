//! Path validation for filesystem operations.
//!
//! Provides a central place to validate paths before they are used for
//! reading or writing. Rejects path traversal (`..` components) and symlink
//! escapes for write operations, without restricting user-selected files to
//! an application root. OmniTools must be able to hash files selected from
//! anywhere the user has filesystem permission to access.

use crate::error::{AppError, AppErrorCode};
use std::path::{Component, Path};

/// Validates that a path is safe to read.
///
/// Rejects `..` path components (traversal) but allows any absolute path
/// the user has permission to access. Symlinks are permitted for reading —
/// a user who selects a symlink intends to hash its target.
pub fn validate_read_path(path: &Path) -> Result<(), AppError> {
    if has_traversal(path) {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!("path traversal detected: {}", path.display()),
        ));
    }
    Ok(())
}

/// Validates that a path is safe to write to.
///
/// Rejects `..` path components (traversal) and symlinks at the destination
/// (symlink escapes could write outside the intended location). Does not
/// restrict the path to an application root — any writable location is
/// permitted.
pub fn validate_write_path(path: &Path) -> Result<(), AppError> {
    if has_traversal(path) {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!("path traversal detected: {}", path.display()),
        ));
    }
    if path.is_symlink() {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!("symlink destination not allowed: {}", path.display()),
        ));
    }
    Ok(())
}

/// Returns `true` if the path contains a `ParentDir` (`..`) component.
fn has_traversal(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn validate_read_path_accepts_normal_absolute_path() {
        assert!(validate_read_path(Path::new("/Users/Aditya/file.txt")).is_ok());
    }

    #[test]
    fn validate_read_path_accepts_path_with_dots_in_filename() {
        // A literal dot in a filename is fine; only `..` as a component is traversal.
        assert!(validate_read_path(Path::new("/Users/Aditya/report.v2.txt")).is_ok());
    }

    #[test]
    fn validate_read_path_rejects_parent_dir_component() {
        let error = validate_read_path(Path::new("/Users/Aditya/../etc/passwd"))
            .expect_err("traversal must be rejected");
        assert!(matches!(error.code, AppErrorCode::InvalidInput));
    }

    #[test]
    fn validate_read_path_rejects_multiple_parent_dirs() {
        let error =
            validate_read_path(Path::new("/a/b/../../c")).expect_err("traversal must be rejected");
        assert!(matches!(error.code, AppErrorCode::InvalidInput));
    }

    #[test]
    fn validate_write_path_accepts_normal_path() {
        assert!(validate_write_path(Path::new("/tmp/checksums.txt")).is_ok());
    }

    #[test]
    fn validate_write_path_rejects_parent_dir_component() {
        let error = validate_write_path(Path::new("/tmp/../etc/passwd"))
            .expect_err("traversal must be rejected");
        assert!(matches!(error.code, AppErrorCode::InvalidInput));
    }

    #[test]
    fn validate_write_path_rejects_symlink() {
        // Create a temp dir with a symlink, then validate against the symlink.
        // Symlink creation may fail on Windows without elevated privileges;
        // in that case the validation logic is still correct and we skip.
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "data").expect("write target");
        let link = dir.path().join("link.txt");

        let created = create_symlink(&target, &link);
        if let Some(link_path) = created {
            let error = validate_write_path(&link_path).expect_err("symlink must be rejected");
            assert!(matches!(error.code, AppErrorCode::InvalidInput));
        }
    }

    #[cfg(unix)]
    fn create_symlink(target: &Path, link: &Path) -> Option<std::path::PathBuf> {
        std::os::unix::fs::symlink(target, link)
            .ok()
            .map(|()| link.to_path_buf())
    }

    #[cfg(windows)]
    fn create_symlink(target: &Path, link: &Path) -> Option<std::path::PathBuf> {
        std::os::windows::fs::symlink_file(target, link)
            .ok()
            .map(|()| link.to_path_buf())
    }

    #[test]
    fn validate_write_path_accepts_non_existent_path() {
        // A path that doesn't exist yet should be valid for writing.
        let path = PathBuf::from("/tmp/nonexistent_checksums_12345.txt");
        assert!(validate_write_path(&path).is_ok());
    }
}
