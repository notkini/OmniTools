//! Canonical, serializable error model shared by every command (PLAN M4 task 4).

use serde::Serialize;
use std::fmt;
use std::io;
use std::path::Path;

/// Closed set of machine-readable codes. Adding a variant requires updating
/// the exhaustive frontend mapping in `src/shared/fs/errors.ts` — that is the
/// contract, not a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppErrorCode {
    NotFound,
    AccessDenied,
    IsADirectory,
    Canceled,
    InvalidInput,
    Io,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Maps an I/O failure into the typed codes, contextualised with the path.
    pub fn from_io_with_path(error: &io::Error, path: &Path) -> Self {
        let code = match error.kind() {
            io::ErrorKind::NotFound => AppErrorCode::NotFound,
            io::ErrorKind::PermissionDenied => AppErrorCode::AccessDenied,
            io::ErrorKind::IsADirectory => AppErrorCode::IsADirectory,
            _ => AppErrorCode::Io,
        };
        Self::new(code, format!("{}: {}", path.display(), error))
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

/// Serializes a code into the exact string the frontend switches on.
#[cfg(test)]
pub(crate) fn code_wire_value(code: AppErrorCode) -> &'static str {
    match code {
        AppErrorCode::NotFound => "not_found",
        AppErrorCode::AccessDenied => "access_denied",
        AppErrorCode::IsADirectory => "is_a_directory",
        AppErrorCode::Canceled => "canceled",
        AppErrorCode::InvalidInput => "invalid_input",
        AppErrorCode::Io => "io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_serializes_with_snake_case_code() {
        let error = AppError::new(AppErrorCode::NotFound, "missing.txt");
        let value = serde_json::to_value(&error).expect("serialize");
        assert_eq!(
            value,
            json!({
                "code": "not_found",
                "message": "missing.txt",
            })
        );
    }

    #[test]
    fn wire_values_match_frontend_codes_exactly() {
        // The frontend's exhaustive map is ["not_found", "access_denied",
        // "is_a_directory", "canceled", "invalid_input", "io"].
        assert_eq!(code_wire_value(AppErrorCode::NotFound), "not_found");
        assert_eq!(code_wire_value(AppErrorCode::AccessDenied), "access_denied");
        assert_eq!(
            code_wire_value(AppErrorCode::IsADirectory),
            "is_a_directory"
        );
        assert_eq!(code_wire_value(AppErrorCode::Canceled), "canceled");
        assert_eq!(code_wire_value(AppErrorCode::InvalidInput), "invalid_input");
        assert_eq!(code_wire_value(AppErrorCode::Io), "io");
    }

    #[test]
    fn io_errors_map_to_typed_codes() {
        let path = Path::new("/nowhere/file.txt");
        let not_found =
            AppError::from_io_with_path(&io::Error::new(io::ErrorKind::NotFound, "gone"), path);
        assert_eq!(not_found.code, AppErrorCode::NotFound);

        let denied = AppError::from_io_with_path(
            &io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            path,
        );
        assert_eq!(denied.code, AppErrorCode::AccessDenied);
    }
}
