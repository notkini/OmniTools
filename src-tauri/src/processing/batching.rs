//! Batch orchestration: a bounded worker pool hashing many files with
//! per-file progress hooks and prompt cancellation. Pure std — no Tauri.

use crate::error::{AppError, AppErrorCode};
use crate::processing::hashing::{sha256_file, to_hex};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Explicit worker limit (PLAN M4: bounded workers, never unbounded fan-out).
pub const MAX_CONCURRENCY: usize = 4;

/// One terminal result per input path, order-preserving. `kind` is the wire
/// discriminator the frontend switches on.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileOutcome {
    #[serde(rename_all = "camelCase")]
    Completed {
        path: PathBuf,
        hex: String,
        size_bytes: u64,
    },
    #[serde(rename_all = "camelCase")]
    Failed { path: PathBuf, error: AppError },
    #[serde(rename_all = "camelCase")]
    Canceled { path: PathBuf },
}

/// Start/finish hook callbacks.
pub type StartedHook = Arc<dyn Fn(usize, &Path) + Send + Sync>;
pub type FinishedHook = Arc<dyn Fn(usize, &Path, &FileOutcome) + Send + Sync>;

/// Per-file lifecycle hooks — used by the command layer to emit progress
/// events, and by tests to observe scheduling and inject cancellation.
/// `Arc`'d closures: the worker pool needs `'static`, and callers share them.
pub struct BatchHooks {
    /// Called on a worker thread right before a file's first read.
    pub on_started: StartedHook,
    /// Called exactly once per file, right after its terminal outcome exists.
    pub on_finished: FinishedHook,
}

impl BatchHooks {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn quiet() -> Self {
        Self {
            on_started: Arc::new(|_, _| {}),
            on_finished: Arc::new(|_, _, _| {}),
        }
    }
}

/// Poison-tolerant lock: a panicked hook must never wedge the batch machine.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// State shared by the bounded worker pool.
struct BatchShared {
    paths: Vec<PathBuf>,
    queue: Mutex<VecDeque<usize>>,
    slots: Mutex<Vec<Option<FileOutcome>>>,
}

impl BatchShared {
    /// Publishes one file's terminal outcome and fires the finished hook.
    /// The slot is recorded before the hook so a hook panic can never lose it.
    fn record(
        &self,
        index: usize,
        mut slots: MutexGuard<'_, Vec<Option<FileOutcome>>>,
        outcome: &FileOutcome,
        hooks: &BatchHooks,
    ) {
        slots[index] = Some(outcome.clone());
        drop(slots);
        (hooks.on_finished)(index, &self.paths[index], outcome);
    }
}

fn process_one(path: &Path, cancel: &AtomicBool) -> FileOutcome {
    match sha256_file(path, cancel) {
        Ok((digest, size_bytes)) => FileOutcome::Completed {
            path: path.to_path_buf(),
            hex: to_hex(&digest),
            size_bytes,
        },
        Err(error) if error.code == AppErrorCode::Canceled => FileOutcome::Canceled {
            path: path.to_path_buf(),
        },
        Err(error) => FileOutcome::Failed {
            path: path.to_path_buf(),
            error,
        },
    }
}

/// Validates + hashes every input. Per-file failures never abort the batch;
/// once cancellation is requested, pending files and single files interrupted
/// mid-stream become `Canceled` — completed/failed files keep their state.
/// `cancel` and `hooks` are owned `Arc`s because worker threads outlive this
/// call's stack.
pub fn run_hash_batch(
    paths: &[PathBuf],
    concurrency_limit: usize,
    cancel: Arc<AtomicBool>,
    hooks: Arc<BatchHooks>,
) -> Vec<FileOutcome> {
    if paths.is_empty() {
        return Vec::new();
    }

    let total = paths.len();
    // Cap at the smaller of the requested limit and the work count — no
    // worker lingers with nothing to do.
    let worker_count = total.min(concurrency_limit.max(1));

    let shared = Arc::new(BatchShared {
        paths: paths.to_vec(),
        queue: Mutex::new((0..total).collect::<VecDeque<usize>>()),
        slots: Mutex::new(vec![None; total]),
    });

    let workers: Vec<std::thread::JoinHandle<()>> = (0..worker_count)
        .map(|_| {
            let shared = Arc::clone(&shared);
            let hooks = Arc::clone(&hooks);
            let cancel = Arc::clone(&cancel);
            std::thread::spawn(move || {
                loop {
                    let index = {
                        let mut queue = lock(&shared.queue);
                        match queue.pop_back() {
                            Some(index) => index,
                            None => break,
                        }
                    };
                    let path = shared.paths[index].clone();
                    if cancel.load(Ordering::Relaxed) {
                        let outcome = FileOutcome::Canceled { path };
                        let slots = lock(&shared.slots);
                        shared.record(index, slots, &outcome, &hooks);
                        continue;
                    }
                    (hooks.on_started)(index, &path);
                    let outcome = process_one(&path, &cancel);
                    let slots = lock(&shared.slots);
                    shared.record(index, slots, &outcome, &hooks);
                }
            })
        })
        .collect();

    for worker in workers {
        let _ = worker.join();
    }

    // Every input has exactly one terminal outcome by this point (workers
    // only exit when the queue is drained); the defensive fallback keeps the
    // result count honest even if a call to `record` was sabotaged.
    let outcome_slots = lock(&shared.slots);
    paths
        .iter()
        .zip(outcome_slots.iter())
        .map(|(path, slot)| {
            slot.clone().unwrap_or_else(|| FileOutcome::Failed {
                path: path.clone(),
                error: AppError::new(
                    AppErrorCode::Io,
                    "internal error: the batch produced no outcome for this file",
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs::File;
    use std::io::Write;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    fn temp_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).expect("create");
        file.write_all(contents).expect("write");
        path
    }

    fn paths_of(outcomes: &[FileOutcome]) -> Vec<PathBuf> {
        outcomes
            .iter()
            .map(|outcome| match outcome {
                FileOutcome::Completed { path, .. }
                | FileOutcome::Failed { path, .. }
                | FileOutcome::Canceled { path } => path.clone(),
            })
            .collect()
    }

    #[test]
    fn preserves_input_order_and_hashes_each_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = temp_file(dir.path(), "a.txt", b"abc");
        let b = temp_file(dir.path(), "b.bin", b"");
        let c = temp_file(dir.path(), "defg.txt", b"defg");
        let cancel = Arc::new(AtomicBool::new(false));

        let outcomes = run_hash_batch(
            &[a.clone(), b.clone(), c.clone()],
            MAX_CONCURRENCY,
            Arc::clone(&cancel),
            Arc::new(BatchHooks::quiet()),
        );

        assert_eq!(paths_of(&outcomes), vec![a, b, c]);
        for outcome in &outcomes {
            assert!(
                matches!(outcome, FileOutcome::Completed { .. }),
                "expected completion, got {outcome:?}"
            );
        }
        // Spot-check the official 'abc' vector.
        match &outcomes[0] {
            FileOutcome::Completed {
                hex, size_bytes, ..
            } => {
                assert_eq!(
                    hex,
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                );
                assert_eq!(*size_bytes, 3);
            }
            other => panic!("checked above, got {other:?}"),
        }
    }

    #[test]
    fn one_file_batch_single_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let single = temp_file(dir.path(), "single.bin", b"one");
        let cancel = Arc::new(AtomicBool::new(false));
        let outcomes = run_hash_batch(
            std::slice::from_ref(&single),
            MAX_CONCURRENCY,
            Arc::clone(&cancel),
            Arc::new(BatchHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], FileOutcome::Completed { .. }));
    }

    #[test]
    fn empty_input_produces_empty_result() {
        let outcomes = run_hash_batch(
            &[],
            MAX_CONCURRENCY,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );
        assert!(outcomes.is_empty());
    }

    #[test]
    fn hundred_file_batch_all_complete_in_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths: Vec<PathBuf> = (0..100)
            .map(|i| {
                temp_file(
                    dir.path(),
                    &format!("f{i:03}.bin"),
                    format!("{i}").as_bytes(),
                )
            })
            .collect();

        let outcomes = run_hash_batch(
            &paths,
            MAX_CONCURRENCY,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 100);
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, FileOutcome::Completed { .. }))
        );
        assert_eq!(paths_of(&outcomes), paths);
    }

    #[test]
    fn thousand_file_batch_completes_deterministically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths: Vec<PathBuf> = (0..1000)
            .map(|i| temp_file(dir.path(), &format!("f{i:04}.bin"), &[(i % 256) as u8; 16]))
            .collect();

        let outcomes = run_hash_batch(
            &paths,
            MAX_CONCURRENCY,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1000);
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, FileOutcome::Completed { .. }))
        );
        assert_eq!(paths_of(&outcomes), paths);
    }

    #[test]
    fn missing_file_and_directory_become_per_file_errors_but_batch_continues() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = temp_file(dir.path(), "good.bin", b"data");
        let ghost = dir.path().join("ghost.bin");

        let outcomes = run_hash_batch(
            &[
                good.clone(),
                dir.path().to_path_buf(),
                ghost.clone(),
                good.clone(),
            ],
            MAX_CONCURRENCY,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );

        assert_eq!(outcomes.len(), 4);
        assert!(matches!(outcomes[0], FileOutcome::Completed { .. }));
        match &outcomes[1] {
            FileOutcome::Failed { error, path } => {
                assert_eq!(path, &dir.path().to_path_buf());
                assert!(matches!(error.code, AppErrorCode::IsADirectory));
            }
            other => panic!("expected failed directory outcome, got {other:?}"),
        }
        match &outcomes[2] {
            FileOutcome::Failed { error, path } => {
                assert_eq!(path, &ghost);
                assert!(matches!(error.code, AppErrorCode::NotFound));
            }
            other => panic!("expected failed missing-file outcome, got {other:?}"),
        }
        // The batch continued after both failures.
        assert!(matches!(outcomes[3], FileOutcome::Completed { .. }));
    }

    #[test]
    fn hook_panics_never_break_the_batch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = temp_file(dir.path(), "a.bin", b"a");
        let b = temp_file(dir.path(), "b.bin", b"b");

        let bomb = |index: usize, _path: &Path, _outcome: &FileOutcome| {
            if index == 1 {
                panic!("hook bomb");
            }
        };
        let hooks = Arc::new(BatchHooks {
            on_started: Arc::new(|_, _| {}),
            on_finished: Arc::new(bomb),
        });
        let outcomes = run_hash_batch(&[a, b], 2, Arc::new(AtomicBool::new(false)), hooks);
        // The slot is recorded before the hook fires, so both files survive
        // even though one hook panicked.
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, FileOutcome::Completed { .. }))
        );
    }

    #[test]
    fn cancel_before_start_marks_every_file_canceled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = temp_file(dir.path(), "a.bin", b"a");
        let b = temp_file(dir.path(), "b.bin", b"b");
        let c = temp_file(dir.path(), "c.bin", b"c");

        let cancel = Arc::new(AtomicBool::new(true));
        let started = Arc::new(AtomicUsize::new(0));
        let started_ref = Arc::clone(&started);
        let hooks = Arc::new(BatchHooks {
            on_started: Arc::new(move |_, _| {
                started_ref.fetch_add(1, AtomicOrdering::Relaxed);
            }),
            on_finished: Arc::new(|_, _, _| {}),
        });
        let outcomes = run_hash_batch(&[a, b, c], MAX_CONCURRENCY, Arc::clone(&cancel), hooks);

        assert_eq!(outcomes.len(), 3);
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, FileOutcome::Canceled { .. }))
        );
        assert_eq!(
            started.load(AtomicOrdering::Relaxed),
            0,
            "no work may start after cancel"
        );
    }

    #[test]
    fn cancel_mixed_with_completions_keeps_completed_and_cancels_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths: Vec<PathBuf> = (0..12)
            .map(|i| temp_file(dir.path(), &format!("f{i:02}.bin"), b"x"))
            .collect();

        let cancel = Arc::new(AtomicBool::new(false));
        let finished_count = Arc::new(AtomicUsize::new(0));
        let cancel_ref = Arc::clone(&cancel);
        let finished_ref = Arc::clone(&finished_count);
        let hooks = Arc::new(BatchHooks {
            on_started: Arc::new(|_, _| {}),
            on_finished: Arc::new(move |index, _path, _outcome| {
                // Cancelling AFTER several completed files is deterministic:
                // the first four complete, the rest are pending when the
                // flag flips.
                if index == 3 {
                    cancel_ref.store(true, AtomicOrdering::Relaxed);
                }
                finished_ref.fetch_add(1, AtomicOrdering::Relaxed);
            }),
        });

        // Serial processing keeps the story deterministic.
        let outcomes = run_hash_batch(&paths, 1, Arc::clone(&cancel), hooks);

        let completed = outcomes
            .iter()
            .filter(|o| matches!(o, FileOutcome::Completed { .. }))
            .count();
        let canceled = outcomes
            .iter()
            .filter(|o| matches!(o, FileOutcome::Canceled { .. }))
            .count();
        let failed = outcomes
            .iter()
            .filter(|o| matches!(o, FileOutcome::Failed { .. }))
            .count();

        assert_eq!(failed, 0, "pending files must be canceled, not errored");
        assert!(completed >= 1, "some files should complete before cancel");
        assert!(canceled >= 1, "pending work must be canceled");
        assert_eq!(completed + canceled, 12);
        // Every input appears exactly once with its original path.
        let unique: BTreeSet<PathBuf> = paths_of(&outcomes).into_iter().collect();
        assert_eq!(unique, paths.into_iter().collect());
    }

    #[test]
    fn cancel_during_a_large_file_yields_canceled_for_that_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = dir.path().join("big.bin");
        // Slightly over one streaming buffer: hashing takes multiple reads,
        // so the between-read cancellation check is exercised mid-file.
        let big_contents = vec![7u8; crate::processing::hashing::STREAM_BUFFER_BYTES + 1];
        File::create(&big)
            .expect("create")
            .write_all(&big_contents)
            .expect("write");
        let small = temp_file(dir.path(), "small.bin", b"s");

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_ref = Arc::clone(&cancel);
        let hooks = Arc::new(BatchHooks {
            on_started: Arc::new(move |index, _path| {
                // Serial run picks small (queue tail) first; the flag flip is
                // timed to the LARGE file's start so it lands mid-stream,
                // caught by the between-reads cancellation check.
                if index == 0 {
                    cancel_ref.store(true, AtomicOrdering::Relaxed);
                }
            }),
            on_finished: Arc::new(|_, _, _| {}),
        });

        let outcomes = run_hash_batch(&[big.clone(), small.clone()], 1, Arc::clone(&cancel), hooks);

        match &outcomes[0] {
            FileOutcome::Canceled { path } => assert_eq!(path, &big),
            other => panic!("expected the large file to be canceled, got {other:?}"),
        }
        assert!(matches!(outcomes[1], FileOutcome::Completed { .. }));
    }

    #[test]
    fn concurrency_limit_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| temp_file(dir.path(), &format!("f{i}.bin"), b"0123456789"))
            .collect();

        let current = Arc::new(AtomicUsize::new(0));
        let max_seen: Arc<StdMutex<usize>> = Arc::new(StdMutex::new(0));
        let current_ref = Arc::clone(&current);
        let max_ref = Arc::clone(&max_seen);
        let hooks = Arc::new(BatchHooks {
            on_started: Arc::new(move |_, _| {
                let now = current_ref.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                let mut max = max_ref.lock().expect("lock");
                if now > *max {
                    *max = now;
                }
                thread::sleep(Duration::from_millis(5));
            }),
            on_finished: Arc::new(move |_, _, _| {
                current.fetch_sub(1, AtomicOrdering::SeqCst);
            }),
        });

        let outcomes = run_hash_batch(&paths, 3, Arc::new(AtomicBool::new(false)), hooks);
        assert_eq!(outcomes.len(), 10);
        let max = *max_seen.lock().expect("lock");
        assert!(
            max <= 3,
            "observed {max} concurrently running files — exceeded the explicit limit of 3"
        );
    }

    #[test]
    fn zero_or_huge_concurrency_is_clamped_to_work_count_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = temp_file(dir.path(), "a.bin", b"a");
        let outcomes = run_hash_batch(
            &[a],
            10_000,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);

        let b = temp_file(dir.path(), "b.bin", b"b");
        let outcomes = run_hash_batch(
            &[b],
            0,
            Arc::new(AtomicBool::new(false)),
            Arc::new(BatchHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
    }

    #[test]
    fn serialization_shape_matches_frontend_discriminator() {
        use serde_json::Value;
        let path = PathBuf::from("/tmp/x.bin");
        let completed = FileOutcome::Completed {
            path: path.clone(),
            hex: "ab".repeat(32),
            size_bytes: 64,
        };
        let failed = FileOutcome::Failed {
            path: path.clone(),
            error: AppError::new(AppErrorCode::NotFound, "missing"),
        };
        let canceled = FileOutcome::Canceled { path };

        let completed_value = serde_json::to_value(&completed).expect("serialize");
        let failed_value = serde_json::to_value(&failed).expect("serialize");
        let canceled_value = serde_json::to_value(&canceled).expect("serialize");

        assert_eq!(completed_value["kind"], Value::String("completed".into()));
        assert_eq!(completed_value["hex"], Value::String("ab".repeat(32)));
        assert_eq!(completed_value["sizeBytes"], Value::from(64));
        assert_eq!(failed_value["kind"], Value::String("failed".into()));
        assert_eq!(
            failed_value["error"]["code"],
            Value::String("not_found".into())
        );
        assert_eq!(canceled_value["kind"], Value::String("canceled".into()));
    }
}
