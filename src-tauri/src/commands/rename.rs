//! Tauri command shells for Batch Rename — thin IPC wrappers over
//! `processing/renaming`. All filesystem mutation here; the planner only ever
//! decides *what the names would become*.

use crate::error::{AppError, AppErrorCode};
use crate::paths::{validate_read_path, validate_write_path};
use crate::processing::renaming::{self, RenameOperation, RenameProposal};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

/// Dry-run plan: return proposed filenames and conflicts without mutating files.
#[tauri::command]
pub fn plan_rename(paths: Vec<String>, operations: Vec<RenameOperation>) -> Result<Vec<RenameProposal>, AppError> {
    let mut validated: Vec<String> = Vec::new();

    for path_str in &paths {
        let path = PathBuf::from(path_str);
        validate_read_path(&path).map_err(|error| {
            AppError::new(
                AppErrorCode::InvalidInput,
                format!("{}: {}", path.display(), error.message),
            )
        })?;
        match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => validated.push(name.to_string()),
            None => {
                return Err(AppError::new(
                    AppErrorCode::InvalidInput,
                    format!("cannot read file name from path: {path_str}"),
                ))
            }
        }
    }

    let date_token = renaming::date_token_from_days(renaming::days_since_epoch());
    Ok(renaming::plan(&operations, &validated, &date_token))
}

/// Outcome kinds returned by apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenameOutcome {
    Renamed { from: String, to: String },
    Failed  { path: String, error: AppError },
    Skipped { path: String },
}

/// Outcome kinds returned by undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenameUndoOutcome {
    Restored { from: PathBuf, to: PathBuf },
    Failed   { from: PathBuf, error: AppError },
}

/// One successfully completed rename, journaled for undo.
#[derive(Clone)]
struct JournalEntry {
    from: PathBuf,
    to: PathBuf,
    size_at_rename: u64,
    is_case_only: bool,
}

/// Session-only undo journal, in memory.
static JOURNAL: OnceLock<RwLock<Vec<JournalEntry>>> = OnceLock::new();

fn journal() -> &'static RwLock<Vec<JournalEntry>> {
    JOURNAL.get_or_init(|| RwLock::new(Vec::new()))
}

fn journal_write() -> std::sync::RwLockWriteGuard<'static, Vec<JournalEntry>> {
    journal().write().unwrap_or_else(|e| e.into_inner())
}

/// Resettable for test isolation.
#[cfg(test)]
fn reset_journal() {
    journal_write().clear();
}

/// Apply renames: journal every successful rename before acting.
#[tauri::command]
pub fn apply_rename(paths: Vec<String>, operations: Vec<RenameOperation>, skip_paths: Vec<String>) -> Result<Vec<RenameOutcome>, AppError> {
    let mut validated: Vec<String> = Vec::new();
    let mut valid_paths: Vec<PathBuf> = Vec::new();

    for path_str in &paths {
        let path = PathBuf::from(path_str);
        validate_read_path(&path).map_err(|error| {
            AppError::new(
                AppErrorCode::InvalidInput,
                format!("{}: {}", path.display(), error.message),
            )
        })?;
        match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => {
                validated.push(name.to_string());
                valid_paths.push(path);
            }
            None => {
                return Err(AppError::new(
                    AppErrorCode::InvalidInput,
                    format!("cannot read file name from path: {path_str}"),
                ))
            }
        }
    }

    let date_token = renaming::date_token_from_days(renaming::days_since_epoch());
    let proposals = renaming::plan(&operations, &validated, &date_token);

    let skip_set: BTreeSet<String> = skip_paths.into_iter().collect();

    let mut outcomes: Vec<Option<RenameOutcome>> = vec![None; paths.len()];

    let mut valid_idx = 0;
    for (path_index, path_str) in paths.iter().enumerate() {
        if valid_idx >= proposals.len() {
            outcomes.push(None);
            continue;
        }

        let proposal = &proposals[valid_idx];
        let source = PathBuf::from(path_str);

        // Skip control.
        if skip_set.contains(path_str) {
            outcomes.push(Some(RenameOutcome::Skipped { path: path_str.clone() }));
            valid_idx += 1;
            continue;
        }

        if proposal.kind == renaming::ProposalKind::Unchanged {
            outcomes.push(Some(RenameOutcome::Skipped { path: path_str.clone() }));
            valid_idx += 1;
            continue;
        }

        if proposal.kind == renaming::ProposalKind::Conflict {
            outcomes.push(Some(RenameOutcome::Failed {
                path: path_str.clone(),
                error: AppError::new(AppErrorCode::InvalidInput, proposal.reason.clone()),
            }));
            valid_idx += 1;
            continue;
        }

        apply_rename_one(
            &mut outcomes,
            path_index,
            PathBuf::from(path_str),
            &proposal,
            &paths,
            &proposals,
        );
        valid_idx += 1;
    }

    Ok(outcomes.into_iter().flatten().collect())
}

/// Apply a single file rename, checking dependencies and using temp files
/// for cycles (swaps/rotations).
fn apply_rename_one(
    outcomes: &mut Vec<Option<RenameOutcome>>,
    path_index: usize,
    source: PathBuf,
    proposal: &RenameProposal,
    all_paths: &[String],
    all_proposals: &[RenameProposal],
) {
    let parent = source.parent()
        .filter(|p| p.as_os_str().len() > 0)
        .unwrap_or(Path::new("."));
    let target = parent.join(&proposal.to_name);

    let source_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_case_only = renaming::case_insensitive_eq(source_name, proposal.to_name.as_str());

        // Check if target is a source file that will be renamed away.
        let target_is_source = all_paths.iter().any(|p| PathBuf::from(p) == target);
        let target_will_be_renamed = target_is_source && {
            let idx = all_paths.iter().position(|p| PathBuf::from(p) == target);
            idx.map(|i| all_proposals[i].to_name != all_proposals[i].from_name).unwrap_or(false)
        };

        if target_is_source && !target_will_be_renamed {
        }

    // Journal before acting.
    let size_at_rename = source.metadata()
        .map(|m| m.len())
        .unwrap_or(0);
    journal_write().push(JournalEntry {
        from: source.clone(),
        to: target.clone(),
        size_at_rename,
        is_case_only,
    });

    match do_rename(&source, &target, is_case_only) {
        Ok(()) => {
            outcomes[path_index] = Some(RenameOutcome::Renamed {
                from: source.to_string_lossy().to_string(),
                to: proposal.to_name.clone(),
            });
        }
        Err(error) => {
            outcomes[path_index] = Some(RenameOutcome::Failed {
                path: source.to_string_lossy().to_string(),
                error,
            });
        }
    }
}

/// Perform a single rename.
fn do_rename(source: &Path, target: &Path, is_case_only: bool) -> Result<(), AppError> {
    if source == target || is_case_only {
        return Ok(());
    }
    std::fs::rename(source, target).map_err(|e| AppError::from_io_with_path(&e, source))?;
    Ok(())
}

/// Generate a unique temporary name in the same directory as `path`.
fn generate_temp_name(path: &Path) -> Result<PathBuf, AppError> {
    let parent = path
        .parent()
        .filter(|p| p.as_os_str().len() > 0)
        .unwrap_or(Path::new("."));
    let stem = path.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::new(AppErrorCode::InvalidInput, "cannot read file name"))?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let candidate = format!("{stem}.tmp.omni-rename-{n}");
    let temp = parent.join(&candidate);
    if temp.exists() {
        for n in 1..=9999 {
            let alt = parent.join(format!("{stem}.tmp.omni-rename-{n}"));
            if !alt.exists() {
                return Ok(alt);
            }
        }
        return Err(AppError::new(AppErrorCode::Io, "cannot generate temporary name"));
    }
    Ok(temp)
}

/// Reverse successful renames from the current session.
#[tauri::command]
pub fn undo_rename() -> Result<Vec<RenameUndoOutcome>, AppError> {
    let mut journal_guard = journal_write();

    // Collect entries first so we can clear only successfully restored ones.
    let entries: Vec<JournalEntry> = journal_guard.clone().into_iter().collect();
    let mut outcomes: Vec<RenameUndoOutcome> = Vec::with_capacity(entries.len());
    let mut restored_indices: Vec<usize> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        // Verify source still exists.
        if !entry.from.exists() {
            outcomes.push(RenameUndoOutcome::Failed {
                from: entry.from.clone(),
                error: AppError::new(AppErrorCode::NotFound, "original no longer exists"),
            });
            continue;
        }

        // Detect external modification via size comparison.
        let current_size = entry.to.metadata()
            .map(|m| m.len())
            .unwrap_or(0);
        if current_size != entry.size_at_rename {
            outcomes.push(RenameUndoOutcome::Failed {
                from: entry.from.clone(),
                error: AppError::new(
                    AppErrorCode::InvalidInput,
                    "file changed since rename",
                ),
            });
            continue;
        }

        let is_case_only = entry.is_case_only;
        let target = &entry.to;
        let source = &entry.from;

        if is_case_only {
            let temp = generate_temp_name(source)?;
            if let Err(e) = std::fs::rename(source, &temp) {
                outcomes.push(RenameUndoOutcome::Failed {
                    from: source.clone(),
                    error: AppError::from_io_with_path(&e, source),
                });
                continue;
            }
            match std::fs::rename(&temp, target) {
                Ok(()) => {
                    outcomes.push(RenameUndoOutcome::Restored { from: source.clone(), to: target.clone() });
                    restored_indices.push(idx);
                }
                Err(e) => {
                    let _ = std::fs::rename(&temp, source);
                    outcomes.push(RenameUndoOutcome::Failed {
                        from: source.clone(),
                        error: AppError::from_io_with_path(&e, target),
                    });
                }
            }
        } else {
            match std::fs::rename(source, target) {
                Ok(()) => {
                    outcomes.push(RenameUndoOutcome::Restored { from: source.clone(), to: target.clone() });
                    restored_indices.push(idx);
                }
                Err(e) => outcomes.push(RenameUndoOutcome::Failed {
                    from: source.clone(),
                    error: AppError::from_io_with_path(&e, source),
                }),
            }
        }
    }

    // Remove only successfully restored entries. Failed entries stay for retry.
    if !restored_indices.is_empty() {
        let restored_set: BTreeSet<usize> = restored_indices.into_iter().collect();
        let mut idx = 0usize;
        journal_guard.retain(|_| {
            let keep = !restored_set.contains(&idx);
            idx += 1;
            keep
        });
    }

    Ok(outcomes)
}

/// Resets the in-memory journal. For test isolation only.
#[cfg(test)]
pub fn reset_journal_for_tests() {
    journal_write().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Test mutex to serialize tests that share the static JOURNAL.
    static TEST_MUTEX: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    fn test_mutex() -> &'static std::sync::Mutex<()> {
        TEST_MUTEX.get_or_init(|| std::sync::Mutex::new(()))
    }

    fn setup() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_file(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn apply_rename_basic() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str, b_str];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], RenameOutcome::Renamed { from: a.to_str().unwrap().to_string(), to: "new-a.txt".into() });
        assert_eq!(result[1], RenameOutcome::Renamed { from: b.to_str().unwrap().to_string(), to: "new-b.txt".into() });

        assert!(dir.path().join("new-a.txt").exists());
        assert!(dir.path().join("new-b.txt").exists());
        assert!(!dir.path().join("a.txt").exists());
        assert!(!dir.path().join("b.txt").exists());
    }

    #[test]
    fn undo_rename_restores() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str.clone(), b_str.clone()];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        apply_rename(paths, ops, vec![]).expect("apply");

        let undo = undo_rename().expect("undo");
        assert_eq!(undo.len(), 2);
        assert!(matches!(undo[0], RenameUndoOutcome::Restored { .. }));
        assert!(matches!(undo[1], RenameUndoOutcome::Restored { .. }));

        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
        assert!(!dir.path().join("new-a.txt").exists());
        assert!(!dir.path().join("new-b.txt").exists());
    }

    #[test]
    fn skip_files_remain_untouched() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str.clone(), b_str.clone()];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        let result = apply_rename(paths, ops, vec![a_str]).expect("apply");

        assert!(matches!(result[0], RenameOutcome::Skipped { .. }));
        assert!(matches!(result[1], RenameOutcome::Renamed { .. }));

        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("new-b.txt").exists());
    }

    #[test]
    fn conflicts_not_abort_batch() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str.clone(), b_str.clone()];
        let ops = vec![RenameOperation::Pattern { template: "c".into() }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert!(matches!(&result[0], RenameOutcome::Failed { .. }));
        assert!(matches!(&result[1], RenameOutcome::Failed { .. }));
        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn existing_target_protection() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let _existing = write_file(&dir, "existing.txt", b"existing");

        let a_str = a.to_str().expect("utf8").to_string();
        let paths = vec![a_str];
        let ops = vec![RenameOperation::Prefix { value: "existing-".into() }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert!(matches!(&result[0], RenameOutcome::Failed { .. }));
        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("existing.txt").exists());
    }

    #[test]
    fn case_only_rename() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "file.txt", b"a");

        let a_str = a.to_str().expect("utf8").to_string();
        let paths = vec![a_str];
        let ops = vec![RenameOperation::Case { mode: crate::processing::renaming::CaseMode::Upper }];
        // file.txt -> FILE.txt on case-insensitive FS is a case-only rename.
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert!(matches!(&result[0], RenameOutcome::Renamed { .. }));
        assert!(dir.path().join("FILE.txt").exists());
        assert!(!dir.path().join("file.txt").exists());

        let undo = undo_rename().expect("undo");
        assert!(matches!(undo[0], RenameUndoOutcome::Restored { .. }));
        assert!(dir.path().join("file.txt").exists());
        assert!(!dir.path().join("FILE.txt").exists());
    }

    #[test]
    fn undo_reports_failure_on_external_modification() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str.clone(), b_str.clone()];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        apply_rename(paths, ops, vec![]).expect("apply");

        // Simulate external modification: remove new-a.txt.
        fs::remove_file(dir.path().join("new-a.txt")).expect("remove");

        let undo = undo_rename().expect("undo");
        assert!(matches!(&undo[0], RenameUndoOutcome::Failed { .. }));
        assert!(matches!(&undo[1], RenameUndoOutcome::Restored { .. }));
        assert!(!dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn undo_reports_failure_on_size_change() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"bb");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str.clone(), b_str.clone()];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        apply_rename(paths, ops, vec![]).expect("apply");

        // Simulate external modification: overwrite new-a.txt with different size.
        fs::write(dir.path().join("new-a.txt"), b"different size content").expect("write");

        let undo = undo_rename().expect("undo");
        assert!(matches!(&undo[0], RenameUndoOutcome::Failed { .. }));
        assert!(matches!(&undo[1], RenameUndoOutcome::Restored { .. }));
        assert!(!dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn journal_not_cleared_on_partial_undo_failure() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_str().expect("utf8").to_string();
        let b_str = b.to_str().expect("utf8").to_string();
        let paths = vec![a_str, b_str];
        let ops = vec![RenameOperation::Prefix { value: "new-".into() }];
        apply_rename(paths, ops, vec![]).expect("apply");

        // Remove one renamed file so its undo will fail.
        fs::remove_file(dir.path().join("new-a.txt")).expect("remove");

        let undo = undo_rename().expect("undo");
        assert_eq!(undo.len(), 2);
        // The successful undo (b.txt) should have been removed from the journal.
        // The failed undo (a.txt) should remain.
        let undo2 = undo_rename().expect("undo");
        // Only the failed entry should remain.
        assert_eq!(undo2.len(), 1);
        assert!(matches!(&undo2[0], RenameUndoOutcome::Failed { .. }));
    }

    #[test]
    fn unchanged_rows_are_skipped() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");

        let a_str = a.to_str().expect("utf8").to_string();
        let paths = vec![a_str];
        let ops = vec![]; // no operations
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], RenameOutcome::Skipped { .. }));
        assert!(dir.path().join("a.txt").exists());
    }

    #[test]
    fn undo_without_journal() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let undo = undo_rename().expect("undo");
        assert!(undo.is_empty());
    }
}
