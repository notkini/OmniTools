//! Tauri command shells — thin IPC wrappers over `processing/`. The only
//! logic here is job-id bookkeeping and event emission. No business logic.

use crate::error::{AppError, AppErrorCode};
use crate::paths::{validate_read_path, validate_write_path};
use crate::processing::batching::{BatchHooks, FileOutcome, MAX_CONCURRENCY, run_hash_batch};
use crate::processing::hashing::compare_files as compare_files_streaming;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// Progress event name — the single canonical channel the queue store
/// subscribes to (before invoking `hash_files`).
pub const HASH_PROGRESS_EVENT: &str = "omnitools://hash-progress";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HashProgressPhase {
    Started,
    Completed,
    Error,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashProgressPayload {
    pub job_id: String,
    pub path: PathBuf,
    pub phase: HashProgressPhase,
    pub hex: Option<String>,
    pub size_bytes: Option<u64>,
    pub code: Option<AppErrorCode>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareOutcome {
    pub identical: bool,
}

/// Live batch cancellation flags (per job id).
static JOB_CANCELS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn job_cancels() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    JOB_CANCELS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Hashes the given files, emitting one started + one terminal event per
/// file. Returns the authoritative per-file outcomes in input order.
#[tauri::command]
pub fn hash_files(app: tauri::AppHandle, job_id: String, paths: Vec<String>) -> Vec<FileOutcome> {
    use tauri::Emitter;

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
        registry.insert(job_id.clone(), Arc::clone(&cancel));
    }

    // Validate paths before hashing. Paths failing validation become
    // per-file failures without aborting the batch. Does not restrict
    // user-selected files to an application root.
    let total = paths.len();
    let mut valid_indices = Vec::new();
    let mut valid_paths = Vec::new();
    let mut failures: Vec<(usize, FileOutcome)> = Vec::new();

    for (index, path_str) in paths.into_iter().enumerate() {
        let path = PathBuf::from(path_str);
        match validate_read_path(&path) {
            Ok(()) => {
                valid_indices.push(index);
                valid_paths.push(path);
            }
            Err(error) => {
                failures.push((index, FileOutcome::Failed { path, error }));
            }
        }
    }

    let emitter = app;

    let emitter_for_started = Arc::new(emitter.clone());
    let emitter_for_finished = Arc::new(emitter.clone());
    let event_id_for_started = job_id.clone();
    let event_id_for_finished = job_id.clone();
    let hooks = Arc::new(BatchHooks {
        on_started: Arc::new(move |index, path| {
            let _ = index;
            let _ = emitter_for_started.emit(
                HASH_PROGRESS_EVENT,
                HashProgressPayload {
                    job_id: event_id_for_started.clone(),
                    path: path.to_path_buf(),
                    phase: HashProgressPhase::Started,
                    hex: None,
                    size_bytes: None,
                    code: None,
                    message: None,
                },
            );
        }),
        on_finished: Arc::new(move |index, path, outcome| {
            let _ = index;
            let (phase, hex, size_bytes, code, message) = match outcome {
                FileOutcome::Completed {
                    hex, size_bytes, ..
                } => (
                    HashProgressPhase::Completed,
                    Some(hex.clone()),
                    Some(*size_bytes),
                    None,
                    None,
                ),
                FileOutcome::Failed { error, .. } => (
                    HashProgressPhase::Error,
                    None,
                    None,
                    Some(error.code),
                    Some(error.message.clone()),
                ),
                FileOutcome::Canceled { .. } => {
                    (HashProgressPhase::Canceled, None, None, None, None)
                }
            };
            let _ = emitter_for_finished.emit(
                HASH_PROGRESS_EVENT,
                HashProgressPayload {
                    job_id: event_id_for_finished.clone(),
                    path: path.to_path_buf(),
                    phase,
                    hex,
                    size_bytes,
                    code,
                    message,
                },
            );
        }),
    });

    let batch_outcomes = run_hash_batch(&valid_paths, MAX_CONCURRENCY, cancel, hooks);

    // Merge batch outcomes with pre-validation failures, preserving input order.
    let mut all_outcomes: Vec<Option<FileOutcome>> = vec![None; total];
    for (batch_i, outcome) in batch_outcomes.into_iter().enumerate() {
        let original_index = valid_indices[batch_i];
        all_outcomes[original_index] = Some(outcome);
    }
    for (original_index, outcome) in failures {
        all_outcomes[original_index] = Some(outcome);
    }

    {
        let mut registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
        registry.remove(&job_id);
    }

    all_outcomes.into_iter().flatten().collect()
}

/// Cancels a running batch. In-flight files stop between buffer reads;
/// pending files are marked canceled; completed and failed files keep their
/// terminal states. Returns false if the job already finished.
#[tauri::command]
pub fn hash_cancel(job_id: String) -> bool {
    let registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
    match registry.get(&job_id) {
        Some(cancel) => {
            cancel.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Byte-exact comparison of two files, streamed in bounded memory.
#[tauri::command]
pub fn compare_files(first: String, second: String) -> Result<CompareOutcome, AppError> {
    let identical = compare_files_streaming(
        PathBuf::from(first).as_path(),
        PathBuf::from(second).as_path(),
    )?;
    Ok(CompareOutcome { identical })
}

/// Appends checksum lines into a destination file: writes a temp file in the
/// destination directory, then replaces the destination — removal before
/// rename is the only portable rename-over-existing on Windows.
#[tauri::command]
pub fn export_checksums(destination: String, entries: Vec<String>) -> Result<(), AppError> {
    let destination = PathBuf::from(destination);
    validate_write_path(&destination)?;
    write_checksums_file(&destination, &entries)
}

/// The checksum interchange format, mirroring GNU `sha256sum` binary mode:
///
/// ```text
/// <sha256-hex>  *<absolute path>
/// ```
///
/// - Exactly two spaces separate the digest from the path tag.
/// - `*` marks binary mode (`sha256sum -c` compatible).
/// - FULL absolute paths (GNU semantics) — this is what keeps two files that
///   share a basename unambiguous, and spaces/Unicode safe, because the path
///   is the final field on the line.
/// - One trailing newline.
fn write_checksums_file(destination: &Path, entries: &[String]) -> Result<(), AppError> {
    if entries.is_empty() {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            "no checksums to export",
        ));
    }
    if destination.is_dir() {
        return Err(AppError::new(
            AppErrorCode::IsADirectory,
            format!("{} is a directory", destination.display()),
        ));
    }

    let temp_name = format!(
        "{}.tmp",
        destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "checksums.txt".to_string())
    );
    let temp = match destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => parent.join(temp_name),
        None => PathBuf::from(temp_name),
    };

    let write_result: Result<(), AppError> = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        let mut contents = String::new();
        for entry in entries {
            contents.push_str(entry);
            contents.push('\n');
        }
        file.write_all(contents.as_bytes())?;
        // fsync the temp file so contents survive a crash between write and
        // rename. Claims stay within what is verified: file contents, not
        // directory-entry durability.
        file.sync_all()?;
        drop(file);

        // Windows cannot rename over an existing file: remove first.
        if destination.exists() {
            let _ = std::fs::remove_file(destination);
        }
        std::fs::rename(&temp, destination)?;
        Ok(())
    })();

    if write_result.is_err() {
        // Never leave a stray temp behind.
        let _ = std::fs::remove_file(&temp);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- checksums.txt format + cross-platform replacement ---

    #[test]
    fn checksums_format_two_spaces_star_full_path_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("checksums.txt");
        let line_one = format!(
            "{}  *{}",
            "a".repeat(64),
            dir.path().join("file one.txt").display()
        );
        let line_two = format!(
            "{}  *{}",
            "b".repeat(64),
            dir.path().join("uñicødé 🧰.bin").display()
        );
        write_checksums_file(&destination, &[line_one.clone(), line_two.clone()]).expect("export");

        let contents = std::fs::read_to_string(&destination).expect("read");
        assert!(contents.ends_with('\n'));
        let lines: Vec<&str> = contents.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], line_one);
        assert_eq!(lines[1], line_two);
    }

    #[test]
    fn export_replaces_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("checksums.txt");
        std::fs::write(&destination, "stale previous export").expect("seed stale");

        let line = format!(
            "{}  *{}",
            "c".repeat(64),
            dir.path().join("x.bin").display()
        );
        write_checksums_file(&destination, &[line]).expect("export");

        let contents = std::fs::read_to_string(&destination).expect("read");
        assert!(contents.starts_with(&"c".repeat(64)));
        assert!(!contents.contains("stale"));
        // The temp file never survives.
        assert!(!dir.path().join("checksums.txt.tmp").exists());
    }

    #[test]
    fn export_creates_new_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("brand-new.txt");
        let line = format!(
            "{}  *{}",
            "d".repeat(64),
            dir.path().join("y.bin").display()
        );
        write_checksums_file(&destination, &[line]).expect("export");
        assert!(destination.exists());
    }

    #[test]
    fn same_basename_files_yet_unambiguous_full_paths() {
        let dir_a = tempfile::tempdir().expect("tempdir");
        let dir_b = tempfile::tempdir().expect("tempdir");
        let destination = dir_a.path().join("checksums.txt");
        let line_a = format!(
            "{}  *{}",
            "e".repeat(64),
            dir_a.path().join("data.bin").display()
        );
        let line_b = format!(
            "{}  *{}",
            "f".repeat(64),
            dir_b.path().join("data.bin").display()
        );
        write_checksums_file(&destination, &[line_a.clone(), line_b.clone()]).expect("export");
        let contents = std::fs::read_to_string(&destination).expect("read");
        assert_eq!(contents, format!("{line_a}\n{line_b}\n"));
    }

    #[test]
    fn export_rejects_empty_entries_and_directory_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = write_checksums_file(&dir.path().join("nope.txt"), &[])
            .expect_err("empty entries must fail");
        assert!(matches!(error.code, AppErrorCode::InvalidInput));
        let error = write_checksums_file(dir.path(), &["x".to_string()])
            .expect_err("directory destination must fail");
        assert!(matches!(error.code, AppErrorCode::IsADirectory));
    }
}
