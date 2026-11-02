//! Tauri command shells — thin IPC wrappers over `processing/imaging`. The only
//! logic here is job-id bookkeeping and event emission. No business logic.

use crate::error::AppErrorCode;
use crate::paths::validate_read_path;
use crate::processing::imaging::{
    ImageHooks, ImageOutcome, MAX_IMAGE_HEIGHT, MAX_IMAGE_WIDTH, MAX_TOTAL_PIXELS,
    SINGLE_IMAGE_MEMORY_BUDGET_BYTES, convert_batch,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// Progress event name — the single canonical channel the queue store
/// subscribes to (before invoking `convert_files`).
pub const IMAGE_PROGRESS_EVENT: &str = "omnitools://image-progress";

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

/// Live batch cancellation flags (per job id).
static JOB_CANCELS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn job_cancels() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    JOB_CANCELS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Converts the given files, emitting one started + one terminal event per
/// file. Returns the authoritative per-file outcomes in input order.
#[tauri::command]
pub fn convert_files(
    app: tauri::AppHandle,
    job_id: String,
    paths: Vec<String>,
    output_dir: String,
    opts: crate::processing::imaging::ConvertOpts,
) -> Vec<ImageOutcome> {
    use tauri::Emitter;

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut registry = job_cancels().lock().unwrap_or_else(PoisonError::into_inner);
        registry.insert(job_id.clone(), Arc::clone(&cancel));
    }

    // Validate the output directory before processing any file.
    let output_dir = PathBuf::from(output_dir);
    if let Err(error) = crate::processing::imaging::validate_output_dir(&output_dir) {
        return vec![ImageOutcome::Failed {
            path: PathBuf::new(),
            error,
        }];
    }

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
        &output_dir,
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

#[cfg(test)]
mod tests {
    use crate::processing::imaging::{ConvertOpts, ImageFormat};
    use serde_json::Value;

    #[test]
    fn convert_opts_serializes_camel_case() {
        let opts = ConvertOpts {
            output_format: ImageFormat::WebP,
            quality: Some(85),
            resize: None,
        };
        let value = serde_json::to_value(&opts).expect("serialize");
        assert_eq!(value["outputFormat"], Value::String("webp".into()));
        assert_eq!(value["quality"], Value::from(85));
        assert!(value["resize"].is_null());
    }
}
