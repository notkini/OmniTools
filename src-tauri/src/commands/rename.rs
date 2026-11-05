//! Tauri command shells for Batch Rename — thin IPC wrappers over
//! `processing/renaming`. All filesystem mutation lives here; the planner
//! only ever decides *what the names would become*.
//!
//! Safety rules (PLAN §M5 tool 2):
//! - Nothing is ever renamed onto an existing file: immediately before every
//!   mutation the target's vacancy is re-checked (`std::fs::rename` would
//!   silently overwrite on Unix and, with REPLACE_EXISTING, on Windows).
//! - Renames whose targets are held by other pending batch members execute in
//!   dependency order; genuine cycles (name swaps/rotations) are broken by
//!   parking one row at a unique temp name in its own directory.
//! - Case-only renames go through a two-step temp rename so they really
//!   rename on every filesystem instead of silently claiming success.
//! - The journal records only completed mutations: entries produced before a
//!   failed first hop are removed again, so undo never replays a rename that
//!   never happened. Files are parked (never lost) when a later hop fails.
//! - Undo replays the journal in reverse completion order, refuses to
//!   restore externally modified files, and keeps failed entries retryable.

use crate::error::{AppError, AppErrorCode};
use crate::paths::validate_read_path;
use crate::processing::renaming::{
    self, ProposalKind, RenameOperation, RenameProposal, case_insensitive_eq,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

/// Dry-run plan: proposed filenames and conflicts, never mutating anything.
/// On-disk occupancy of every `Ok` target is checked against a fresh
/// directory listing, so external collisions surface before the user
/// commits. Names of rows that themselves rename away are excluded from the
/// listing (the apply layer vacates them first).
#[tauri::command]
pub fn plan_rename(
    paths: Vec<String>,
    operations: Vec<RenameOperation>,
) -> Result<Vec<RenameProposal>, AppError> {
    let rows = collect_rows(&paths)?;
    let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    let date_token = renaming::date_token_from_days(renaming::days_since_epoch());
    let mut proposals = renaming::plan(&operations, &names, &date_token);
    downgrade_disk_conflicts(&rows, &mut proposals)?;
    // DRAMATIZATION DELETED: kept purely fluent — see `Row` docs below.
    Ok(proposals)
}

/// One validated input row: absolute path plus its file name.
pub(crate) struct Row {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}

fn collect_rows(paths: &[String]) -> Result<Vec<Row>, AppError> {
    paths
        .iter()
        .map(|path_str| {
            let path = PathBuf::from(path_str);
            validate_read_path(&path).map_err(|error| {
                AppError::new(
                    AppErrorCode::InvalidInput,
                    format!("{}: {}", path.display(), error.message),
                )
            })?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_string())
                .ok_or_else(|| {
                    AppError::new(
                        AppErrorCode::InvalidInput,
                        format!("cannot read file name from path: {path_str}"),
                    )
                })?;
            Ok(Row { path, name })
        })
        .collect()
}

fn downgrade_disk_conflicts(
    rows: &[Row],
    proposals: &mut [RenameProposal],
) -> Result<(), AppError> {
    // Rows grouped per parent directory so each directory is listed once.
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let parent = row.path.parent().unwrap_or_else(|| Path::new("."));
        grouped
            .entry(parent.to_string_lossy().to_string())
            .or_default()
            .push(index);
    }

    for (dir_key, indices) in grouped {
        // lowercased existing name → every on-disk spelling with that case
        // key (on case-sensitive filesystems several can coexist).
        let mut occupancy: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for entry in std::fs::read_dir(&dir_key)
            .map_err(|error| AppError::from_io_with_path(&error, Path::new(&dir_key)))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let actual = entry.file_name().to_string_lossy().to_string();
            occupancy
                .entry(actual.to_lowercase())
                .or_default()
                .push(actual);
        }
        for &index in &indices {
            let proposal = &proposals[index];
            if proposal.kind == ProposalKind::Ok
                && !case_insensitive_eq(&proposal.from_name, &proposal.to_name)
                && let Some(spellings) = occupancy.get_mut(&proposal.from_name.to_lowercase())
            {
                spellings.retain(|actual| !case_insensitive_eq(actual, &proposal.from_name));
                if spellings.is_empty() {
                    occupancy.remove(&proposal.from_name.to_lowercase());
                }
            }
        }
        for &index in &indices {
            if proposals[index].kind != ProposalKind::Ok {
                continue;
            }
            if let Some(actual) = occupancy
                .get(&proposals[index].to_name.to_lowercase())
                .and_then(|spellings| spellings.last())
            {
                proposals[index].kind = ProposalKind::Conflict;
                proposals[index].reason = format!("target already exists on disk: '{actual}'");
            }
        }
    }
    Ok(())
}

/// Outcome kinds returned by apply. `to` is an absolute path so the frontend
/// can keep its file list in sync with the disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenameOutcome {
    Renamed { from: String, to: String },
    Failed { path: String, error: AppError },
    Skipped { path: String },
}

/// Outcome kinds returned by undo. `from` = the restored original path,
/// `to` = where the file was before undo (the frontend swaps its entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenameUndoOutcome {
    Restored { from: PathBuf, to: PathBuf },
    Failed { from: PathBuf, error: AppError },
}

/// One journal entry per original source path: the file was verifiably at
/// `to` with `size_at_rename` bytes when this entry was last written, so undo
/// can refuse externally modified files.
#[derive(Debug, Clone)]
struct JournalEntry {
    from: PathBuf,
    to: PathBuf,
    size_at_rename: u64,
}

/// Session-only undo journal, in memory (lost on restart by design).
static JOURNAL: OnceLock<RwLock<Vec<JournalEntry>>> = OnceLock::new();

fn journal() -> &'static RwLock<Vec<JournalEntry>> {
    JOURNAL.get_or_init(|| RwLock::new(Vec::new()))
}

fn journal_write() -> std::sync::RwLockWriteGuard<'static, Vec<JournalEntry>> {
    journal().write().unwrap_or_else(|e| e.into_inner())
}

/// Records (or moves) the single entry for one original source path, so the
/// temp→final hops of one file never stack intermediate positions.
fn journal_upsert(from: &Path, to: &Path, size_at_rename: u64) {
    let mut guard = journal_write();
    if let Some(existing) = guard.iter_mut().find(|entry| entry.from == from) {
        existing.to = to.to_path_buf();
        existing.size_at_rename = size_at_rename;
    } else {
        guard.push(JournalEntry {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            size_at_rename,
        });
    }
}

/// Drops the entry for a source path — used after failed first hops, where
/// no real mutation happened and nothing may remain undoable.
fn journal_remove(from: &Path) {
    journal_write().retain(|entry| entry.from != from);
}

/// Resettable for test isolation.
#[cfg(test)]
fn reset_journal() {
    journal_write().clear();
}

/// Apply renames. Conflicts never abort the batch; skipped files keep their
/// names; exactly one outcome per input row, in input order.
#[tauri::command]
pub fn apply_rename(
    paths: Vec<String>,
    operations: Vec<RenameOperation>,
    skip_paths: Vec<String>,
) -> Result<Vec<RenameOutcome>, AppError> {
    let rows = collect_rows(&paths)?;
    let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    let date_token = renaming::date_token_from_days(renaming::days_since_epoch());
    let proposals = renaming::plan(&operations, &names, &date_token);
    Ok(apply_proposals(
        &rows,
        &proposals,
        &skip_paths.into_iter().collect(),
    ))
}

/// Core executor, separated from the command so dependency ordering, temp
/// cycles and journal interactions are unit-testable without the IPC layer.
pub(crate) fn apply_proposals(
    rows: &[Row],
    proposals: &[RenameProposal],
    skip_set: &BTreeSet<String>,
) -> Vec<RenameOutcome> {
    // Phase 0 — classify every row without touching the filesystem.
    let mut outcomes: Vec<Option<RenameOutcome>> = Vec::with_capacity(rows.len());
    let mut pending: Vec<PendingMove> = Vec::new();
    // Dependency graph index: lowercased current disk name → pending index.
    let mut holders: BTreeMap<String, usize> = BTreeMap::new();

    for (index, row) in rows.iter().enumerate() {
        let path_string = row.path.to_string_lossy().to_string();
        let outcome = if skip_set.contains(&path_string) {
            Some(RenameOutcome::Skipped { path: path_string })
        } else {
            match &proposals[index].kind {
                ProposalKind::Unchanged => Some(RenameOutcome::Skipped { path: path_string }),
                ProposalKind::Conflict => Some(RenameOutcome::Failed {
                    path: path_string,
                    error: AppError::new(
                        AppErrorCode::InvalidInput,
                        proposals[index].reason.clone(),
                    ),
                }),
                ProposalKind::Ok => {
                    let parent = row.path.parent().unwrap_or_else(|| Path::new("."));
                    let target = parent.join(&proposals[index].to_name);
                    holders.insert(row.name.to_lowercase(), pending.len());
                    pending.push(PendingMove {
                        original: row.path.clone(),
                        current: row.path.clone(),
                        target,
                        row: index,
                        done: false,
                    });
                    // Resolved by the execution phase below.
                    None
                }
            }
        };
        outcomes.push(outcome);
    }

    // Phase 1 — execute pending moves in dependency order.
    for move_index in 0..pending.len() {
        let mut visiting: BTreeSet<usize> = BTreeSet::new();
        execute_pending(
            &mut pending,
            &mut holders,
            &mut outcomes,
            move_index,
            &mut visiting,
        );
    }

    // Honesty guard: every row must carry a real outcome after execution.
    outcomes
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| match outcome {
            Some(outcome) => outcome,
            None => RenameOutcome::Failed {
                path: rows[index].path.to_string_lossy().to_string(),
                error: AppError::new(
                    AppErrorCode::Io,
                    "internal state error: row was not executed",
                ),
            },
        })
        .collect()
}

/// One pending batch member; `current` tracks where the file verifiably is
/// (it may live at a temp name after a cycle break). `row` is the outcome
/// slot this member reports into (rows before it may be skipped or failed).
struct PendingMove {
    original: PathBuf,
    current: PathBuf,
    target: PathBuf,
    row: usize,
    done: bool,
}

/// Row-failure helper — a free function so the mutable slices stay usable
/// around every call site.
fn fail_row(
    pending: &[PendingMove],
    outcomes: &mut [Option<RenameOutcome>],
    index: usize,
    error: AppError,
) {
    outcomes[pending[index].row] = Some(RenameOutcome::Failed {
        path: pending[index].original.to_string_lossy().to_string(),
        error,
    });
}

/// Executes one pending move, first unwinding any pending source that holds
/// its target name. When the dependency chain runs into an ancestor (a
/// cycle), the current row is parked at a unique temp name, which vacates
/// the slot the ancestor needs; the outer pass finishes parked rows.
fn execute_pending(
    pending: &mut [PendingMove],
    holders: &mut BTreeMap<String, usize>,
    outcomes: &mut [Option<RenameOutcome>],
    index: usize,
    visiting: &mut BTreeSet<usize>,
) {
    if pending[index].done || visiting.contains(&index) {
        return;
    }
    visiting.insert(index);

    // The file must still exist where we believe it is.
    if std::fs::symlink_metadata(&pending[index].current).is_err() {
        holders.remove(&current_name_key(pending, index));
        fail_row(
            pending,
            outcomes,
            index,
            AppError::new(AppErrorCode::NotFound, "file no longer exists"),
        );
        pending[index].done = true;
        visiting.remove(&index);
        return;
    }

    // Case-only target (the only case-insensitive match for our own name is
    // the file itself): two-step rename; never a silent no-op.
    if case_insensitive_eq(
        &current_name_key(pending, index),
        &file_name_lossy(&pending[index].target),
    ) {
        // perform_two_step owns this row's outcome in full, errors included.
        let _ = perform_two_step(pending, outcomes, index);
        pending[index].done = true;
        visiting.remove(&index);
        return;
    }

    // Dependency unwinding: while another pending source occupies our target
    // name, move that source first. `holders` reflects current disk names.
    loop {
        let occupier = holders
            .get(&file_name_lossy(&pending[index].target).to_lowercase())
            .copied();
        match occupier {
            Some(other) if !pending[other].done => {
                if other == index || visiting.contains(&other) {
                    // Cycle: park this row at a temp name (breaking the loop)
                    // and return. The ancestor's move vacates our target; the
                    // outer pass completes this row afterwards.
                    match park_at_temp(pending, holders, index) {
                        Ok(()) => {}
                        Err(error) => {
                            fail_row(pending, outcomes, index, error);
                            pending[index].done = true;
                            holders.remove(&current_name_key(pending, index));
                        }
                    }
                    visiting.remove(&index);
                    return;
                }
                execute_pending(pending, holders, outcomes, other, visiting);
            }
            _ => break,
        }
    }
    if pending[index].done {
        visiting.remove(&index);
        return;
    }

    // Immediate pre-mutation recheck: never rename onto an existing file.
    if std::fs::symlink_metadata(&pending[index].target).is_ok() {
        holders.remove(&current_name_key(pending, index));
        fail_row(
            pending,
            outcomes,
            index,
            AppError::new(
                AppErrorCode::InvalidInput,
                format!(
                    "target name in use: '{}' was not overwritten",
                    file_name_lossy(&pending[index].target)
                ),
            ),
        );
        pending[index].done = true;
        visiting.remove(&index);
        return;
    }

    // Real mutation. Journal before acting; a failed first hop removes its
    // entry again, so only completed mutations are undoable.
    let size_at_rename = std::fs::metadata(&pending[index].current)
        .map(|m| m.len())
        .unwrap_or(0);
    journal_upsert(
        &pending[index].original,
        &pending[index].current,
        size_at_rename,
    );
    match std::fs::rename(&pending[index].current, &pending[index].target) {
        Ok(()) => {
            let old_key = current_name_key(pending, index);
            pending[index].current = pending[index].target.clone();
            holders.remove(&old_key);
            holders.insert(new_name_key(pending, index), index);
            journal_upsert(
                &pending[index].original,
                &pending[index].target,
                size_at_rename,
            );
            outcomes[pending[index].row] = Some(RenameOutcome::Renamed {
                from: pending[index].original.to_string_lossy().to_string(),
                to: pending[index].target.to_string_lossy().to_string(),
            });
            pending[index].done = true;
        }
        Err(error) => {
            holders.remove(&current_name_key(pending, index));
            if pending[index].current == pending[index].original {
                // First hop failed: nothing moved; nothing to undo.
                journal_remove(&pending[index].original);
            }
            fail_row(
                pending,
                outcomes,
                index,
                AppError::from_io_with_path(&error, &pending[index].current),
            );
            pending[index].done = true;
        }
    }
    visiting.remove(&index);
}

/// Cycle break: move the row's file to a unique temp name in its own
/// directory, record the new position in the journal (a real mutation
/// happened), and leave the row pending for its final hop later.
fn park_at_temp(
    pending: &mut [PendingMove],
    holders: &mut BTreeMap<String, usize>,
    index: usize,
) -> Result<(), AppError> {
    let temp = generate_temp_name(&pending[index].current)?;
    let size_at_rename = std::fs::metadata(&pending[index].current)
        .map(|m| m.len())
        .unwrap_or(0);
    journal_upsert(
        &pending[index].original,
        &pending[index].current,
        size_at_rename,
    );
    std::fs::rename(&pending[index].current, &temp)
        .map_err(|error| AppError::from_io_with_path(&error, &pending[index].current))?;
    let old_key = current_name_key(pending, index);
    pending[index].current = temp.clone();
    holders.remove(&old_key);
    holders.insert(file_name_lossy(&temp).to_lowercase(), index);
    journal_upsert(&pending[index].original, &temp, size_at_rename);
    Ok(())
}

/// Case-only rename: current → unique temp → target, so the change lands on
/// every filesystem. An externally occupied target puts the file back at its
/// source and fails honestly.
fn perform_two_step(
    pending: &mut [PendingMove],
    outcomes: &mut [Option<RenameOutcome>],
    index: usize,
) -> Result<(), AppError> {
    let temp = generate_temp_name(&pending[index].current)?;
    let size_at_rename = std::fs::metadata(&pending[index].current)
        .map(|m| m.len())
        .unwrap_or(0);
    journal_upsert(
        &pending[index].original,
        &pending[index].current,
        size_at_rename,
    );
    if let Err(error) = std::fs::rename(&pending[index].current, &temp) {
        journal_remove(&pending[index].original);
        fail_row(
            pending,
            outcomes,
            index,
            AppError::from_io_with_path(&error, &pending[index].current),
        );
        return Err(AppError::from_io_with_path(&error, &pending[index].current));
    }
    let source = pending[index].current.clone();
    pending[index].current = temp.clone();
    journal_upsert(&pending[index].original, &temp, size_at_rename);

    // The only case-insensitive occupant of the target is the file itself;
    // but on a case-sensitive filesystem another spelling of the same name
    // may exist. Refuse without data loss in that situation.
    if temp != pending[index].target && std::fs::symlink_metadata(&pending[index].target).is_ok() {
        let _ = std::fs::rename(&temp, &source);
        pending[index].current = source;
        journal_remove(&pending[index].original);
        outcomes[pending[index].row] = Some(RenameOutcome::Failed {
            path: pending[index].original.to_string_lossy().to_string(),
            error: AppError::new(
                AppErrorCode::InvalidInput,
                format!(
                    "target name in use: '{}' was not overwritten",
                    file_name_lossy(&pending[index].target)
                ),
            ),
        });
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            "target name in use",
        ));
    }

    if let Err(error) = std::fs::rename(&temp, &pending[index].target) {
        // Leave the file parked at the temp position — the journal already
        // knows, so undo still reaches it.
        fail_row(
            pending,
            outcomes,
            index,
            AppError::from_io_with_path(&error, &temp),
        );
        return Err(AppError::from_io_with_path(&error, &temp));
    }
    pending[index].current = pending[index].target.clone();
    journal_upsert(
        &pending[index].original,
        &pending[index].target,
        size_at_rename,
    );
    outcomes[pending[index].row] = Some(RenameOutcome::Renamed {
        from: pending[index].original.to_string_lossy().to_string(),
        to: pending[index].target.to_string_lossy().to_string(),
    });
    Ok(())
}

fn current_name_key(pending: &[PendingMove], index: usize) -> String {
    file_name_lossy(&pending[index].current).to_lowercase()
}

fn new_name_key(pending: &[PendingMove], index: usize) -> String {
    file_name_lossy(&pending[index].target).to_lowercase()
}

fn file_name_lossy(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Generates a unique temporary name beside `path` (same directory).
fn generate_temp_name(path: &Path) -> Result<PathBuf, AppError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
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
        return Err(AppError::new(
            AppErrorCode::Io,
            "cannot generate temporary name",
        ));
    }
    Ok(temp)
}

/// Undo: restores every journaled file to its exact original name.
///
/// Rename cycles (swaps, rotations) leave original names occupied by *other
/// batch members*, so undo resolves dependencies too: every round restores
/// each entry whose original name is currently free; an entry whose name is
/// held by another still-pending journal entry is deferred to a later round.
/// When a round makes no progress, one blocked file is parked at a unique
/// temp name — breaking the cycle — and the next rounds finish it.
///
/// Entries whose file is missing or was externally modified fail for that
/// row and REMAIN in the journal for retry; restored entries are pruned.
#[tauri::command]
pub fn undo_rename() -> Result<Vec<RenameUndoOutcome>, AppError> {
    enum Status {
        Pending,
        Restored,
        Failed(AppError),
    }

    let mut entries: Vec<JournalEntry> = journal_write().clone();
    let count = entries.len();
    let mut status: Vec<Status> = (0..count).map(|_| Status::Pending).collect();

    // Upfront guards: existence and an unchanged size since the rename.
    for (index, entry) in entries.iter().enumerate() {
        if std::fs::symlink_metadata(&entry.to).is_err() {
            status[index] = Status::Failed(AppError::new(
                AppErrorCode::NotFound,
                "renamed file no longer exists; cannot restore",
            ));
            continue;
        }
        let current_size = std::fs::metadata(&entry.to).map(|m| m.len()).unwrap_or(0);
        if current_size != entry.size_at_rename {
            status[index] = Status::Failed(AppError::new(
                AppErrorCode::InvalidInput,
                "file changed since rename; not overwritten",
            ));
        }
    }

    loop {
        let mut progress = false;

        // One restoration round, latest journal entry first.
        for index in (0..count).rev() {
            if !matches!(status[index], Status::Pending) {
                continue;
            }
            let entry = &entries[index];
            let case_only = entry.from != entry.to
                && case_insensitive_eq(&file_name_lossy(&entry.from), &file_name_lossy(&entry.to));

            if !case_only && std::fs::symlink_metadata(&entry.from).is_ok() {
                // Occupied. Defer only if the occupant is another pending
                // journal entry that will vacate the name when it restores;
                // anything else is an honest refusal, never an overwrite.
                let occupied_by_batch = (0..count).any(|other| {
                    other != index
                        && matches!(status[other], Status::Pending)
                        && entries[other].to == entry.from
                });
                if occupied_by_batch {
                    continue;
                }
                status[index] = Status::Failed(AppError::new(
                    AppErrorCode::InvalidInput,
                    "original name re-occupied since rename; not overwritten",
                ));
                progress = true;
                continue;
            }

            let restore_result = if case_only {
                restore_two_step(entry)
            } else {
                std::fs::rename(&entry.to, &entry.from)
                    .map_err(|error| AppError::from_io_with_path(&error, &entry.to))
            };
            match restore_result {
                Ok(()) => status[index] = Status::Restored,
                Err(error) => status[index] = Status::Failed(error),
            }
            progress = true;
        }

        if progress {
            continue;
        }

        // No round progress: a rename cycle. Park one pending file at a
        // unique temp name so the entry blocking *it* can restore next round.
        match (0..count)
            .rev()
            .find(|&index| matches!(status[index], Status::Pending))
        {
            None => break,
            Some(index) => {
                if let Err(error) = park_entry_at_temp(&mut entries[index]) {
                    // Nothing moved; it stays undoable for a retry.
                    status[index] = Status::Failed(error);
                }
            }
        }
    }

    // Outcomes in reverse journal (reverse completion) order; the journal is
    // rewritten without restored entries, keeping truthful positions for the
    // failures so a later retry works from where things actually are.
    let mut outcomes: Vec<RenameUndoOutcome> = Vec::with_capacity(count);
    let mut kept_entries: Vec<JournalEntry> = Vec::with_capacity(count);
    for (index, entry) in entries.iter().enumerate().rev() {
        match &status[index] {
            Status::Restored => outcomes.push(RenameUndoOutcome::Restored {
                from: entry.from.clone(),
                to: entry.to.clone(),
            }),
            Status::Failed(error) => outcomes.push(RenameUndoOutcome::Failed {
                from: entry.from.clone(),
                error: error.clone(),
            }),
            // The loop above cannot exit while pending entries exist, but an
            // honest fallback keeps the entry retryable rather than lying.
            Status::Pending => outcomes.push(RenameUndoOutcome::Failed {
                from: entry.from.clone(),
                error: AppError::new(AppErrorCode::Io, "undo did not complete for this entry"),
            }),
        }
        if !matches!(status[index], Status::Restored) {
            kept_entries.push(entry.clone());
        }
    }
    kept_entries.reverse();
    *journal_write() = kept_entries;

    Ok(outcomes)
}

/// Cycle break for undo: park the entry's file at a unique temp name in its
/// own directory and record the new position in the entry.
fn park_entry_at_temp(entry: &mut JournalEntry) -> Result<(), AppError> {
    let temp = generate_temp_name(&entry.to)?;
    std::fs::rename(&entry.to, &temp)
        .map_err(|error| AppError::from_io_with_path(&error, &entry.to))?;
    entry.to = temp;
    Ok(())
}

/// Case-only restore through a temp name; on failure the file is put back
/// where it was before the attempt.
fn restore_two_step(entry: &JournalEntry) -> Result<(), AppError> {
    let temp = generate_temp_name(&entry.to)?;
    std::fs::rename(&entry.to, &temp)
        .map_err(|error| AppError::from_io_with_path(&error, &entry.to))?;
    match std::fs::rename(&temp, &entry.from) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Leave the file where it was before this undo attempt.
            let _ = std::fs::rename(&temp, &entry.to);
            Err(AppError::from_io_with_path(&error, &entry.from))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Tests share the static JOURNAL and must run serially.
    static TEST_MUTEX: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

    fn test_mutex() -> &'static std::sync::Mutex<()> {
        TEST_MUTEX.get_or_init(|| std::sync::Mutex::new(()))
    }

    fn setup() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// Case-sensitive existence check: on Windows `Path::exists()` matches
    /// case-insensitively, which would hide a failed case-only rename.
    fn exists_exact(dir: &std::path::Path, name: &str) -> bool {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| entry.file_name().to_string_lossy() == name)
            })
            .unwrap_or(false)
    }

    fn write_file(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("write");
        path
    }

    fn outcome_names(values: &[RenameOutcome]) -> Vec<String> {
        values
            .iter()
            .map(|outcome| match outcome {
                RenameOutcome::Renamed { to, .. } => format!("renamed:{to}"),
                RenameOutcome::Failed { error, .. } => format!("failed:{}", error.message),
                RenameOutcome::Skipped { .. } => "skipped".to_string(),
            })
            .collect()
    }

    fn swap_proposals(dir: &tempfile::TempDir) -> (Vec<Row>, Vec<RenameProposal>) {
        // Hand-built swap (planner allows: each takes the other's vacated
        // name). Row 0 targets row 1's current name and vice versa.
        let rows = vec![
            Row {
                path: dir.path().join("a.txt"),
                name: "a.txt".to_string(),
            },
            Row {
                path: dir.path().join("b.txt"),
                name: "b.txt".to_string(),
            },
        ];
        let proposals = vec![
            RenameProposal {
                from_name: "a.txt".to_string(),
                to_name: "b.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
            RenameProposal {
                from_name: "b.txt".to_string(),
                to_name: "a.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
        ];
        (rows, proposals)
    }

    #[test]
    fn apply_rename_basic() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert_eq!(result.len(), 2);
        assert_eq!(
            outcome_names(&result),
            vec![
                format!("renamed:{}", dir.path().join("new-a.txt").to_string_lossy()),
                format!("renamed:{}", dir.path().join("new-b.txt").to_string_lossy()),
            ],
            "one outcome per input, in input order, absolute `to` paths"
        );

        assert!(dir.path().join("new-a.txt").exists());
        assert!(dir.path().join("new-b.txt").exists());
        assert!(!dir.path().join("a.txt").exists());
        assert!(!dir.path().join("b.txt").exists());
    }

    #[test]
    fn undo_rename_restores_exact_originals() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        apply_rename(paths, ops, vec![]).expect("apply");

        let undo = undo_rename().expect("undo");
        assert_eq!(undo.len(), 2);
        assert!(matches!(undo[0], RenameUndoOutcome::Restored { .. }));
        assert!(matches!(undo[1], RenameUndoOutcome::Restored { .. }));

        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
        assert!(!dir.path().join("new-a.txt").exists());
        assert!(!dir.path().join("new-b.txt").exists());

        // A clean undo drains the journal — the next undo is a no-op.
        let undo2 = undo_rename().expect("undo after drain");
        assert!(undo2.is_empty(), "journal pruned after full restore");
    }

    #[test]
    fn skip_files_remain_untouched() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let a_str = a.to_string_lossy().to_string();
        let paths = vec![a_str.clone(), b.to_string_lossy().to_string()];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        let result = apply_rename(paths, ops, vec![a_str]).expect("apply");

        assert!(matches!(result[0], RenameOutcome::Skipped { .. }));
        assert!(matches!(result[1], RenameOutcome::Renamed { .. }));

        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("new-b.txt").exists());
    }

    #[test]
    fn conflicts_do_not_abort_the_batch() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let _a = write_file(&dir, "a.txt", b"a");
        let _b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            dir.path().join("a.txt").to_string_lossy().to_string(),
            dir.path().join("b.txt").to_string_lossy().to_string(),
        ];
        // Both rows plan identical targets: the earlier row wins the free
        // name, the later row conflicts and the batch continues.
        let ops = vec![RenameOperation::Pattern {
            template: "c".into(),
        }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert!(
            matches!(&result[0], RenameOutcome::Renamed { .. }),
            "planned-earlier row renames onto the free name: {:?}",
            result[0]
        );
        match &result[1] {
            RenameOutcome::Failed { error, .. } => {
                assert!(
                    error.message.contains("duplicate"),
                    "reason: {}",
                    error.message
                );
            }
            other => panic!("later duplicate row must fail honestly: {other:?}"),
        }
        assert!(dir.path().join("c.txt").exists());
        assert!(dir.path().join("b.txt").exists());
        assert!(!dir.path().join("a.txt").exists());
    }

    #[test]
    fn existing_target_is_never_overwritten() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let existing = write_file(&dir, "existing.txt", b"existing");

        let paths = vec![a.to_string_lossy().to_string()];
        // Pattern replaces the stem; extension is kept → "existing.txt".
        let ops = vec![RenameOperation::Pattern {
            template: "existing".into(),
        }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        match &result[0] {
            RenameOutcome::Failed { error, .. } => {
                assert!(
                    error.message.contains("in use"),
                    "reason: {}",
                    error.message
                );
            }
            other => panic!("occupied target must fail honestly: {other:?}"),
        }
        assert!(dir.path().join("a.txt").exists(), "original preserved");
        let _ = existing;
    }

    #[test]
    fn case_only_rename_actually_renames_and_undo_restores() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "file.txt", b"a");

        let paths = vec![a.to_string_lossy().to_string()];
        let ops = vec![RenameOperation::Case {
            mode: crate::processing::renaming::CaseMode::Upper,
        }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        assert!(
            matches!(&result[0], RenameOutcome::Renamed { .. }),
            "case-only rename must be a real rename: {:?}",
            result[0]
        );
        assert!(exists_exact(dir.path(), "FILE.txt"));
        assert!(!exists_exact(dir.path(), "file.txt"));

        let undo = undo_rename().expect("undo");
        assert!(matches!(undo[0], RenameUndoOutcome::Restored { .. }));
        assert!(exists_exact(dir.path(), "file.txt"));
        assert!(!exists_exact(dir.path(), "FILE.txt"));
    }

    #[test]
    fn swap_pair_through_dependency_engine_preserves_both_files() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let _a = write_file(&dir, "a.txt", b"a");
        let _b = write_file(&dir, "b.txt", b"b");

        let (rows, proposals) = swap_proposals(&dir);
        let result = apply_proposals(&rows, &proposals, &BTreeSet::new());

        assert!(matches!(&result[0], RenameOutcome::Renamed { .. }));
        assert!(matches!(&result[1], RenameOutcome::Renamed { .. }));
        // Contents survive the swap under the exchanged names.
        assert_eq!(fs::read(dir.path().join("b.txt")).expect("b"), b"a");
        assert_eq!(fs::read(dir.path().join("a.txt")).expect("a"), b"b");
        assert!(
            !generate_temp_name(&dir.path().join("a.txt"))
                .expect("probe")
                .exists(),
            "no leftovers: the probe temp name must be free"
        );

        // Undo restores both to their exact originals.
        let undo = undo_rename().expect("undo");
        assert!(
            undo.iter()
                .all(|outcome| matches!(outcome, RenameUndoOutcome::Restored { .. }))
        );
        assert_eq!(
            fs::read(dir.path().join("a.txt")).expect("restored a"),
            b"a"
        );
        assert_eq!(
            fs::read(dir.path().join("b.txt")).expect("restored b"),
            b"b"
        );
    }

    #[test]
    fn three_step_rotation_unwinds_with_one_temp_hop() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let _a = write_file(&dir, "a.txt", b"a");
        let _b = write_file(&dir, "b.txt", b"b");
        let _c = write_file(&dir, "c.txt", b"c");

        let rows = vec![
            Row {
                path: dir.path().join("a.txt"),
                name: "a.txt".to_string(),
            },
            Row {
                path: dir.path().join("b.txt"),
                name: "b.txt".to_string(),
            },
            Row {
                path: dir.path().join("c.txt"),
                name: "c.txt".to_string(),
            },
        ];
        let proposals = vec![
            RenameProposal {
                from_name: "a.txt".to_string(),
                to_name: "b.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
            RenameProposal {
                from_name: "b.txt".to_string(),
                to_name: "c.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
            RenameProposal {
                from_name: "c.txt".to_string(),
                to_name: "a.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
        ];
        let result = apply_proposals(&rows, &proposals, &BTreeSet::new());

        assert!(
            result
                .iter()
                .all(|outcome| matches!(outcome, RenameOutcome::Renamed { .. }))
        );
        assert_eq!(fs::read(dir.path().join("b.txt")).expect("b"), b"a");
        assert_eq!(fs::read(dir.path().join("c.txt")).expect("c"), b"b");
        assert_eq!(fs::read(dir.path().join("a.txt")).expect("a"), b"c");

        let undo = undo_rename().expect("undo");
        assert!(
            undo.iter()
                .all(|outcome| matches!(outcome, RenameUndoOutcome::Restored { .. }))
        );
        assert_eq!(
            fs::read(dir.path().join("a.txt")).expect("restored a"),
            b"a"
        );
        assert_eq!(
            fs::read(dir.path().join("b.txt")).expect("restored b"),
            b"b"
        );
        assert_eq!(
            fs::read(dir.path().join("c.txt")).expect("restored c"),
            b"c"
        );
    }

    #[test]
    fn skipping_an_occupier_blocks_the_taker_rather_than_overwriting() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let _a = write_file(&dir, "a.txt", b"a");
        let _b = write_file(&dir, "b.txt", b"b");

        // Row 0 targets "b.txt"; the planner allows it because row 1 would
        // vacate. At apply, row 1 is UNCHANGED (stays) — row 0 must fail
        // against the still-present file instead of overwriting it.
        let rows = vec![
            Row {
                path: dir.path().join("a.txt"),
                name: "a.txt".to_string(),
            },
            Row {
                path: dir.path().join("b.txt"),
                name: "b.txt".to_string(),
            },
        ];
        let proposals = vec![
            RenameProposal {
                from_name: "a.txt".to_string(),
                to_name: "b.txt".to_string(),
                kind: ProposalKind::Ok,
                reason: String::new(),
            },
            RenameProposal {
                from_name: "b.txt".to_string(),
                to_name: "b.txt".to_string(),
                kind: ProposalKind::Unchanged,
                reason: String::new(),
            },
        ];
        let result = apply_proposals(&rows, &proposals, &BTreeSet::new());

        match &result[0] {
            RenameOutcome::Failed { error, .. } => {
                assert!(
                    error.message.contains("in use"),
                    "reason: {}",
                    error.message
                );
            }
            other => panic!("taking a stationary occupier's name must fail: {other:?}"),
        }
        assert!(dir.path().join("a.txt").exists());
        assert!(
            dir.path().join("b.txt").exists(),
            "occupier never overwritten"
        );
    }

    #[test]
    fn file_disappearing_between_plan_and_apply_fails_honestly() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");

        let paths = vec![a.to_string_lossy().to_string()];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        // The file vanishes after planning.
        fs::remove_file(&a).expect("remove");
        let result = apply_rename(paths, ops, vec![]).expect("apply");

        match &result[0] {
            RenameOutcome::Failed { error, .. } => {
                assert!(error.message.contains("no longer exists"));
            }
            other => panic!("vanished source must fail honestly: {other:?}"),
        }
    }

    #[test]
    fn unicode_names_survive_apply_and_undo() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "café — über.txt", b"a");

        let paths = vec![a.to_string_lossy().to_string()];
        let ops = vec![RenameOperation::Prefix {
            value: "Neu ".into(),
        }];
        let result = apply_rename(paths, ops, vec![]).expect("apply");
        assert!(matches!(&result[0], RenameOutcome::Renamed { .. }));
        assert!(dir.path().join("Neu café — über.txt").exists());

        let undo = undo_rename().expect("undo");
        assert!(matches!(undo[0], RenameUndoOutcome::Restored { .. }));
        assert!(dir.path().join("café — über.txt").exists());
    }

    #[test]
    fn undo_refuses_externally_modified_files() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        apply_rename(paths, ops, vec![]).expect("apply");

        // Simulate external modification: overwrite one renamed file.
        fs::write(dir.path().join("new-a.txt"), b"different size content").expect("write");

        let undo = undo_rename().expect("undo");
        assert_eq!(undo.len(), 2);
        // Reverse completion order: b restores first, the modified a refuses.
        assert!(
            matches!(&undo[0], RenameUndoOutcome::Restored { .. }),
            "intact file restores: {:?}",
            undo[0]
        );
        assert!(
            matches!(&undo[1], RenameUndoOutcome::Failed { .. }),
            "externally modified file refuses restore: {:?}",
            undo[1]
        );
        assert!(dir.path().join("b.txt").exists());
        assert!(
            dir.path().join("new-a.txt").exists(),
            "modified file untouched"
        );
    }

    #[test]
    fn journal_keeps_failed_entries_retryable() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let ops = vec![RenameOperation::Prefix {
            value: "new-".into(),
        }];
        apply_rename(paths, ops, vec![]).expect("apply");

        // Remove one renamed file so its undo fails.
        fs::remove_file(dir.path().join("new-a.txt")).expect("remove");

        let undo = undo_rename().expect("undo");
        assert_eq!(undo.len(), 2);

        // Second attempt: only the failed entry remains, retryable.
        let undo2 = undo_rename().expect("retry undo");
        assert_eq!(undo2.len(), 1);
        assert!(matches!(&undo2[0], RenameUndoOutcome::Failed { .. }));

        // Recreate the missing file and retry once more — now it succeeds.
        fs::write(dir.path().join("new-a.txt"), b"a").expect("recreate");
        let undo3 = undo_rename().expect("retry after fixing");
        assert!(matches!(undo3[0], RenameUndoOutcome::Restored { .. }));
        assert!(dir.path().join("a.txt").exists());
        assert!(undo_rename().expect("final").is_empty(), "journal drained");
    }

    #[test]
    fn unchanged_rows_become_skipped_outcomes() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");

        let paths = vec![a.to_string_lossy().to_string()];
        let result = apply_rename(paths, vec![], vec![]).expect("apply");

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], RenameOutcome::Skipped { .. }));
        assert!(dir.path().join("a.txt").exists());
    }

    #[test]
    fn undo_without_journal_is_empty() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let undo = undo_rename().expect("undo");
        assert!(undo.is_empty());
    }

    #[test]
    fn plan_rename_flags_disk_occupied_targets_and_never_mutates() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let _taken = write_file(&dir, "taken.txt", b"taken");

        let paths = vec![a.to_string_lossy().to_string()];
        // Pattern replaces the stem; extension is kept → "taken.txt".
        let ops = vec![RenameOperation::Pattern {
            template: "taken".into(),
        }];
        let proposals = plan_rename(paths, ops).expect("plan");

        assert_eq!(proposals.len(), 1);
        assert!(
            proposals[0].kind == ProposalKind::Conflict,
            "disk-occupied target must be downgraded: {:?}",
            proposals[0]
        );
        assert!(
            proposals[0]
                .reason
                .contains("target already exists on disk: 'taken.txt'")
        );

        // A dry run mutates nothing.
        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("taken.txt").exists());
    }

    #[test]
    fn plan_rename_allows_takeover_of_vacating_names() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_journal();
        let dir = setup();
        let a = write_file(&dir, "a.txt", b"a");
        let b = write_file(&dir, "b.txt", b"b");

        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        // Both rows plan "b.txt"-family collisions: with shared ops the
        // planner conflicts the later duplicate; the vacate-allowance is
        // exercised in the dependency-engine tests above.
        let ops = vec![RenameOperation::Pattern {
            template: "x".into(),
        }];
        let proposals = plan_rename(paths, ops).expect("plan");

        assert_eq!(proposals[0].kind, ProposalKind::Ok);
        assert_eq!(proposals[1].kind, ProposalKind::Conflict);
        assert!(
            proposals[1].reason.contains("duplicate target"),
            "reason: {}",
            proposals[1].reason
        );
    }
}
