//! Tauri command shells — thin IPC wrappers over `processing/imaging`. The only
//! logic here is job-id bookkeeping, staging-directory lifecycle, and event
//! emission. No business logic.

use crate::error::{AppError, AppErrorCode};
use crate::paths::{validate_read_path, validate_write_path};
use crate::processing::imaging::{
    ImageHooks, ImageOutcome, MAX_IMAGE_HEIGHT, MAX_IMAGE_WIDTH, MAX_TOTAL_PIXELS,
    SINGLE_IMAGE_MEMORY_BUDGET_BYTES, convert_batch,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use tempfile::TempDir;

/// Progress event name — the single canonical channel the queue store
/// subscribes to (before invoking `convert_files`).
pub const IMAGE_PROGRESS_EVENT: &str = "omnitools://image-progress";
/// Event name for individual save results.
pub const IMAGE_SAVE_EVENT: &str = "omnitools://image-save";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageProgressPhase {
    Started,
    Completed,
    Error,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageProgressPayload {
    pub job_id: String,
    pub path: PathBuf,
    pub phase: ImageProgressPhase,
    pub input_size_bytes: Option<u64>,
    pub output_size_bytes: Option<u64>,
    pub code: Option<AppErrorCode>,
    pub message: Option<String>,
}

/// Save result per file.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveOutcome {
    #[serde(rename_all = "camelCase")]
    Saved {
        staged_file_id: String,
        destination: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    SaveError {
        staged_file_id: String,
        error: AppError,
    },
}

/// Live batch cancellation flags (per job id).
static JOB_CANCELS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn job_cancels() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    JOB_CANCELS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Tauri-managed staging directory for converted outputs.
/// Holds a `TempDir` for the lifetime of the application session.
pub struct StagingManager {
    pub dir: Mutex<Option<TempDir>>,
}

impl StagingManager {
    /// Ensure the staging directory exists, creating it on first use.
    fn ensure_dir(&self) -> Result<PathBuf, AppError> {
        let mut guard = self.dir.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard =
                Some(TempDir::new().map_err(|e| AppError::new(AppErrorCode::Io, format!("{e}")))?);
        }
        Ok(guard.as_ref().unwrap().path().to_path_buf())
    }

    /// Resolve a staged_file_id to its absolute path, verifying it is inside
    /// the staging directory (no traversal, no symlink escapes).
    fn resolve_staged(&self, staged_file_id: &str) -> Result<PathBuf, AppError> {
        let staging_dir = self.ensure_dir()?;
        let path = staging_dir.join(staged_file_id);
        let canonical = path.canonicalize().map_err(|_| {
            AppError::new(
                AppErrorCode::NotFound,
                format!("staged file not found: {staged_file_id}"),
            )
        })?;
        let staging_canonical = staging_dir
            .canonicalize()
            .map_err(|e| AppError::new(AppErrorCode::Io, format!("{e}")))?;
        if !canonical.starts_with(&staging_canonical) {
            return Err(AppError::new(
                AppErrorCode::InvalidInput,
                "staged file path escapes staging directory",
            ));
        }
        Ok(canonical)
    }
}

/// Tauri state type for the staging manager.
pub type StagingState = StagingManager;

/// Converts the given files to the staging directory, emitting one started +
/// one terminal event per file. Returns the authoritative per-file outcomes
/// in input order. Each completed outcome carries an opaque `staged_file_id`
/// used by save commands.
#[tauri::command]
pub fn convert_files(
    app: tauri::AppHandle,
    job_id: String,
    paths: Vec<String>,
    state: tauri::State<'_, StagingState>,
    opts: crate::processing::imaging::ConvertOpts,
) -> Vec<ImageOutcome> {
    use tauri::Emitter;

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
        registry.insert(job_id.clone(), Arc::clone(&cancel));
    }

    // Ensure staging directory exists.
    let staging_dir = match state.ensure_dir() {
        Ok(d) => d,
        Err(error) => {
            return vec![ImageOutcome::Failed {
                path: PathBuf::new(),
                error,
            }];
        }
    };

    // Validate each input path. Paths failing validation become per-file
    // failures without aborting the batch.
    let total = paths.len();
    let mut valid_indices = Vec::new();
    let mut valid_paths = Vec::new();
    let mut failures: Vec<(usize, ImageOutcome)> = Vec::new();

    for (index, path_str) in paths.into_iter().enumerate() {
        let path = PathBuf::from(path_str);
        match validate_read_path(&path) {
            Ok(()) => {
                valid_indices.push(index);
                valid_paths.push(path);
            }
            Err(error) => {
                failures.push((index, ImageOutcome::Failed { path, error }));
            }
        }
    }

    let emitter = app;
    let emitter_for_started = Arc::new(emitter.clone());
    let emitter_for_finished = Arc::new(emitter.clone());
    let event_id_for_started = job_id.clone();
    let event_id_for_finished = job_id.clone();
    let hooks = Arc::new(ImageHooks {
        on_started: Arc::new(move |index, path| {
            let _ = index;
            let _ = emitter_for_started.emit(
                IMAGE_PROGRESS_EVENT,
                ImageProgressPayload {
                    job_id: event_id_for_started.clone(),
                    path: path.to_path_buf(),
                    phase: ImageProgressPhase::Started,
                    input_size_bytes: None,
                    output_size_bytes: None,
                    code: None,
                    message: None,
                },
            );
        }),
        on_finished: Arc::new(move |index, path, outcome| {
            let _ = index;
            let (phase, input_size_bytes, output_size_bytes, code, message) = match outcome {
                ImageOutcome::Completed {
                    input_size_bytes,
                    output_size_bytes,
                    ..
                } => (
                    ImageProgressPhase::Completed,
                    Some(*input_size_bytes),
                    Some(*output_size_bytes),
                    None,
                    None,
                ),
                ImageOutcome::Failed { error, .. } => (
                    ImageProgressPhase::Error,
                    None,
                    None,
                    Some(error.code),
                    Some(error.message.clone()),
                ),
                ImageOutcome::Canceled { .. } => {
                    (ImageProgressPhase::Canceled, None, None, None, None)
                }
            };
            let _ = emitter_for_finished.emit(
                IMAGE_PROGRESS_EVENT,
                ImageProgressPayload {
                    job_id: event_id_for_finished.clone(),
                    path: path.to_path_buf(),
                    phase,
                    input_size_bytes,
                    output_size_bytes,
                    code,
                    message,
                },
            );
        }),
    });

    let batch_outcomes = convert_batch(
        &valid_paths,
        &staging_dir,
        &opts,
        4, // MAX_CONCURRENCY — same bounded pool as the hasher
        cancel,
        hooks,
    );

    // Merge batch outcomes with pre-validation failures, preserving input order.
    let mut all_outcomes: Vec<Option<ImageOutcome>> = vec![None; total];
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

/// Cancels a running batch. Pending files become canceled; in-flight files
/// stop at the next checkpoint. Returns false if the job already finished.
#[tauri::command]
pub fn image_cancel(job_id: String) -> bool {
    let registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
    match registry.get(&job_id) {
        Some(cancel) => {
            cancel.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Expose limits for the frontend to clamp UI controls.
#[tauri::command]
pub fn image_limits() -> serde_json::Value {
    serde_json::json!({
        "maxWidth": MAX_IMAGE_WIDTH,
        "maxHeight": MAX_IMAGE_HEIGHT,
        "maxTotalPixels": MAX_TOTAL_PIXELS,
        "singleImageMemoryBudgetBytes": SINGLE_IMAGE_MEMORY_BUDGET_BYTES,
    })
}

/// Save a single staged converted output to a user-chosen destination.
/// The staged file is identified by `staged_file_id` (opaque, Rust-managed).
/// The destination is validated and published with no-clobber semantics.
#[tauri::command]
pub fn save_converted(
    app: tauri::AppHandle,
    state: tauri::State<'_, StagingState>,
    staged_file_id: String,
    destination: String,
) -> Result<(), AppError> {
    use tauri::Emitter;

    let staging_path = state.resolve_staged(&staged_file_id)?;
    let destination = PathBuf::from(destination);

    // Validate the destination path (no traversal).
    validate_write_path(&destination)?;

    // Validate destination parent is a directory.
    if let Some(parent) = destination.parent()
        && !parent.is_dir()
    {
        return Err(AppError::new(
            AppErrorCode::NotFound,
            format!(
                "destination parent is not a directory: {}",
                parent.display()
            ),
        ));
    }

    // Publish with no-clobber: fail if destination already exists.
    if destination.exists() {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!("destination already exists: {}", destination.display()),
        ));
    }

    // Copy staged file to destination.
    match std::fs::copy(&staging_path, &destination) {
        Ok(_) => {
            let _ = app.emit(
                IMAGE_SAVE_EVENT,
                serde_json::json!({
                    "stagedFileId": staged_file_id,
                    "destination": destination,
                    "kind": "saved",
                }),
            );
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                IMAGE_SAVE_EVENT,
                serde_json::json!({
                    "stagedFileId": staged_file_id,
                    "destination": destination,
                    "kind": "saveError",
                    "error": {
                        "code": "io",
                        "message": e.to_string(),
                    },
                }),
            );
            Err(AppError::new(AppErrorCode::Io, format!("{e}")))
        }
    }
}

/// Save all staged converted outputs to a user-chosen destination directory.
/// Each file gets a unique name (_1, _2, … suffixes on collision).
/// Returns per-file `Vec<SaveOutcome>`.
#[tauri::command]
pub fn save_all_converted(
    app: tauri::AppHandle,
    state: tauri::State<'_, StagingState>,
    dest_dir: String,
    staged_file_ids: Vec<String>,
) -> Vec<SaveOutcome> {
    use tauri::Emitter;

    let dest_dir = PathBuf::from(dest_dir);

    // Validate destination directory.
    if let Err(e) = validate_write_path(&dest_dir) {
        return staged_file_ids
            .into_iter()
            .map(|id| SaveOutcome::SaveError {
                staged_file_id: id,
                error: e.clone(),
            })
            .collect();
    }
    if !dest_dir.is_dir() {
        let error = AppError::new(
            AppErrorCode::NotFound,
            format!("destination is not a directory: {}", dest_dir.display()),
        );
        return staged_file_ids
            .into_iter()
            .map(|id| SaveOutcome::SaveError {
                staged_file_id: id,
                error: error.clone(),
            })
            .collect();
    }

    let mut results: Vec<SaveOutcome> = Vec::with_capacity(staged_file_ids.len());
    let mut used_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for staged_file_id in staged_file_ids {
        let staging_path = match state.resolve_staged(&staged_file_id) {
            Ok(p) => p,
            Err(e) => {
                results.push(SaveOutcome::SaveError {
                    staged_file_id: staged_file_id.clone(),
                    error: e,
                });
                continue;
            }
        };

        // Determine output name: preserve original staged file name,
        // with _1, _2, … suffixes on collision.
        let file_name = staging_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let mut output_name = file_name.to_string();
        let mut counter = 1u32;
        while used_names.contains(&output_name) || dest_dir.join(&output_name).exists() {
            output_name = format!("{}_{counter}", file_name);
            counter += 1;
            if counter > 9999 {
                output_name = format!("{file_name}_{counter}");
                break;
            }
        }
        used_names.insert(output_name.clone());

        let destination = dest_dir.join(&output_name);
        match std::fs::copy(&staging_path, &destination) {
            Ok(_) => {
                let _ = app.emit(
                    IMAGE_SAVE_EVENT,
                    serde_json::json!({
                        "stagedFileId": staged_file_id,
                        "destination": destination,
                        "kind": "saved",
                    }),
                );
                results.push(SaveOutcome::Saved {
                    staged_file_id,
                    destination,
                });
            }
            Err(e) => {
                let error = AppError::new(AppErrorCode::Io, format!("{e}"));
                let _ = app.emit(
                    IMAGE_SAVE_EVENT,
                    serde_json::json!({
                        "stagedFileId": staged_file_id,
                        "destination": destination,
                        "kind": "saveError",
                        "error": {
                            "code": "io",
                            "message": e.to_string(),
                        },
                    }),
                );
                results.push(SaveOutcome::SaveError {
                    staged_file_id,
                    error,
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use serde_json::Value;

    #[test]
    fn convert_opts_serializes_camel_case() {
        let opts = crate::processing::imaging::ConvertOpts {
            output_format: crate::processing::imaging::ImageFormat::WebP,
            quality: Some(85),
            resize: None,
        };
        let value = serde_json::to_value(&opts).expect("serialize");
        assert_eq!(value["outputFormat"], Value::String("webp".into()));
        assert_eq!(value["quality"], Value::from(85));
        assert!(value["resize"].is_null());
    }

    #[test]
    fn save_outcome_serializes() {
        let saved = SaveOutcome::Saved {
            staged_file_id: "staged-0".to_string(),
            destination: PathBuf::from("/tmp/out.png"),
        };
        let value = serde_json::to_value(&saved).expect("serialize");
        assert_eq!(value["kind"], Value::String("saved".into()));
        assert_eq!(value["stagedFileId"], Value::String("staged-0".into()));

        let error = SaveOutcome::SaveError {
            staged_file_id: "staged-1".to_string(),
            error: AppError::new(AppErrorCode::Io, "copy failed"),
        };
        let value = serde_json::to_value(&error).expect("serialize");
        assert_eq!(value["kind"], Value::String("saveError".into()));
        assert_eq!(value["error"]["code"], Value::String("io".into()));
    }
}
