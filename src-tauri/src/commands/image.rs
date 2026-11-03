//! Tauri command shells — thin IPC wrappers over `processing/imaging`. The only
//! logic here is job-id bookkeeping, staging-directory lifecycle, and event
//! emission. No business logic.

use crate::error::{AppError, AppErrorCode};
use crate::paths::{validate_read_path, validate_write_path};
use crate::processing::imaging::{
    ImageFormat, ImageHooks, ImageOutcome, MAX_IMAGE_HEIGHT, MAX_IMAGE_WIDTH, MAX_TOTAL_PIXELS,
    SINGLE_IMAGE_MEMORY_BUDGET_BYTES, convert_batch,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    pub format: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
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

/// A registered staged output: its file name inside its per-batch directory.
/// The file name is the human-readable `<input-stem>.<ext>` the conversion
/// produced — saves use it so published files keep meaningful names.
#[derive(Debug, Clone)]
struct StagedEntry {
    batch_dir: PathBuf,
    file_name: String,
}

/// A fully validated staged file: absolute path + published friendly name.
#[derive(Debug, Clone)]
pub struct ResolvedStaged {
    pub path: PathBuf,
    pub file_name: String,
}

/// Tauri-managed staging area for converted outputs.
///
/// Layout:
///   <root TempDir>/batch-<n>/<input-stem>.<ext>
///
/// Each `convert_files` batch gets its own subdirectory, so identical file
/// names in different batches can never overwrite each other's staged files.
/// The `entries` map translates the opaque `staged_file_id` handed to the
/// frontend into `(batch_dir, file_name)` — the frontend never learns or
/// controls real filesystem paths, so traversal-by-id is impossible by
/// construction (ids never join the path directly).
///
/// The root is a `TempDir` for the lifetime of the application session, so
/// everything staged is purged on exit; `discard_staged` removes batches
/// eagerly when the user clears results.
pub struct StagingManager {
    root: Mutex<Option<TempDir>>,
    next_batch: std::sync::atomic::AtomicU64,
    entries: Mutex<HashMap<String, StagedEntry>>,
}

impl Default for StagingManager {
    fn default() -> Self {
        Self {
            root: Mutex::new(None),
            next_batch: std::sync::atomic::AtomicU64::new(0),
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl StagingManager {
    fn lock_entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, StagedEntry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Create a fresh per-batch directory under the staging root.
    /// The root `TempDir` is created on first use and purged on app exit.
    pub fn new_batch(&self) -> Result<PathBuf, AppError> {
        let mut guard = self.root.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard =
                Some(TempDir::new().map_err(|e| AppError::new(AppErrorCode::Io, format!("{e}")))?);
        }
        let root = guard
            .as_ref()
            .expect("root just created")
            .path()
            .to_path_buf();
        drop(guard);

        let n = self
            .next_batch
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let batch_dir = root.join(format!("batch-{n}"));
        std::fs::create_dir(&batch_dir).map_err(|e| AppError::from_io_with_path(&e, &batch_dir))?;
        Ok(batch_dir)
    }

    /// Register a completed conversion: `staged_file_id` → file name inside
    /// `batch_dir`. The name must be a plain file name — this is internally
    /// generated, but the guard keeps the invariant explicit and testable.
    pub fn register(
        &self,
        staged_file_id: &str,
        batch_dir: &Path,
        file_name: &str,
    ) -> Result<(), AppError> {
        if file_name.is_empty()
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name == ".."
            || file_name == "."
        {
            return Err(AppError::new(
                AppErrorCode::InvalidInput,
                format!("invalid staged file name: {file_name}"),
            ));
        }
        let mut entries = self.lock_entries();
        entries.insert(
            staged_file_id.to_string(),
            StagedEntry {
                batch_dir: batch_dir.to_path_buf(),
                file_name: file_name.to_string(),
            },
        );
        Ok(())
    }

    /// Resolve a staged_file_id to its absolute path, verifying the file is
    /// still inside its registered batch directory (no traversal possible:
    /// the id is only ever a map key, never a path component).
    pub fn resolve_staged(&self, staged_file_id: &str) -> Result<ResolvedStaged, AppError> {
        let entry = {
            let entries = self.lock_entries();
            entries.get(staged_file_id).cloned()
        };
        let Some(entry) = entry else {
            return Err(AppError::new(
                AppErrorCode::NotFound,
                format!(
                    "staged file not found (it may have been cleared after a previous save round): {staged_file_id}"
                ),
            ));
        };
        let path = entry.batch_dir.join(&entry.file_name);
        let canonical = path.canonicalize().map_err(|_| {
            AppError::new(
                AppErrorCode::NotFound,
                format!("staged file no longer exists: {staged_file_id}"),
            )
        })?;
        let batch_canonical = entry
            .batch_dir
            .canonicalize()
            .map_err(|e| AppError::from_io_with_path(&e, &entry.batch_dir))?;
        if !canonical.starts_with(&batch_canonical) {
            return Err(AppError::new(
                AppErrorCode::InvalidInput,
                "staged file path escapes staging directory",
            ));
        }
        Ok(ResolvedStaged {
            path: canonical,
            file_name: entry.file_name,
        })
    }

    /// Discard staged outputs (e.g. after Clear results). Removes the map
    /// entries and deletes each affected batch directory if no registered
    /// entry still references it. Returns the number of ids discarded.
    pub fn discard(&self, staged_file_ids: &[String]) -> Result<usize, AppError> {
        let mut affected: Vec<PathBuf> = Vec::new();
        let mut discarded = 0usize;
        {
            let mut entries = self.lock_entries();
            for id in staged_file_ids {
                if let Some(entry) = entries.remove(id) {
                    discarded += 1;
                    if !affected.contains(&entry.batch_dir) {
                        affected.push(entry.batch_dir);
                    }
                }
            }
            for dir in &affected {
                // Only delete directories no remaining entry references.
                let still_referenced = entries.values().any(|e| e.batch_dir == *dir);
                if !still_referenced && dir.is_dir() {
                    std::fs::remove_dir_all(dir)
                        .map_err(|e| AppError::from_io_with_path(&e, dir))?;
                }
            }
        }
        Ok(discarded)
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

    // Create this batch's own staging subdirectory. Converted files are
    // named `<input-stem>.<ext>` inside it — registration maps the opaque
    // staged_file_ids handed back below to these real file names.
    let staging_dir = match state.new_batch() {
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
                    format: None,
                    width: None,
                    height: None,
                    code: None,
                    message: None,
                },
            );
        }),
        on_finished: Arc::new(move |index, path, outcome| {
            let _ = index;
            let (phase, input_size_bytes, output_size_bytes, format, width, height, code, message) =
                match outcome {
                    ImageOutcome::Completed {
                        input_size_bytes,
                        output_size_bytes,
                        format,
                        width,
                        height,
                        ..
                    } => {
                        let format_str = match format {
                            ImageFormat::Png => "png".to_string(),
                            ImageFormat::Jpeg => "jpeg".to_string(),
                            ImageFormat::WebP => "webp".to_string(),
                        };
                        (
                            ImageProgressPhase::Completed,
                            Some(*input_size_bytes),
                            Some(*output_size_bytes),
                            Some(format_str),
                            Some(*width),
                            Some(*height),
                            None,
                            None,
                        )
                    }
                    ImageOutcome::Failed { error, .. } => (
                        ImageProgressPhase::Error,
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(error.code),
                        Some(error.message.clone()),
                    ),
                    ImageOutcome::Canceled { .. } => (
                        ImageProgressPhase::Canceled,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                };
            let _ = emitter_for_finished.emit(
                IMAGE_PROGRESS_EVENT,
                ImageProgressPayload {
                    job_id: event_id_for_finished.clone(),
                    path: path.to_path_buf(),
                    phase,
                    input_size_bytes,
                    output_size_bytes,
                    format,
                    width,
                    height,
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
    // Completed outcomes are registered here so the save commands can resolve
    // staged_file_id → (batch dir, real file name). A registration failure
    // (never expected) becomes an honest per-file error, not a dead row.
    let mut all_outcomes: Vec<Option<ImageOutcome>> = vec![None; total];
    for (batch_i, outcome) in batch_outcomes.into_iter().enumerate() {
        let original_index = valid_indices[batch_i];
        let outcome = match &outcome {
            ImageOutcome::Completed {
                staged_file_id,
                output_path,
                ..
            } => {
                let file_name = output_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("image");
                match state.register(staged_file_id, &staging_dir, file_name) {
                    Ok(()) => outcome,
                    Err(error) => ImageOutcome::Failed {
                        path: output_path.clone(),
                        error,
                    },
                }
            }
            _ => outcome,
        };
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

/// Longest side of generated queue-row thumbnails.
const THUMBNAIL_MAX_DIMENSION: u32 = 128;

/// Minimal standard base64 encoder — hand-rolled to keep the dependency
/// set minimal (PLAN licence discipline). Used only for data-URL thumbnails.
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Builds a small PNG data-URL preview of a staged converted file for its
/// queue row. Decode + resize + encode all happen in Rust (no web-layer
/// file access, no new capability).
#[tauri::command]
pub fn image_thumbnail(
    state: tauri::State<'_, StagingState>,
    staged_file_id: String,
) -> Result<String, AppError> {
    let staged = state.resolve_staged(&staged_file_id)?;
    let img = image::ImageReader::open(&staged.path)
        .map_err(|e| AppError::from_io_with_path(&e, &staged.path))?
        .with_guessed_format()
        .map_err(|e| AppError::new(AppErrorCode::Io, format!("{e}")))?
        .decode()
        .map_err(|e| AppError::new(AppErrorCode::Io, format!("thumbnail decode failed: {e}")))?;

    // Longest-side fit with cheap sampling — a 128px preview, nothing more.
    let thumb = img.thumbnail(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION);
    let mut buffer = Vec::new();
    thumb
        .write_to(
            &mut std::io::Cursor::new(&mut buffer),
            image::ImageFormat::Png,
        )
        .map_err(|e| AppError::new(AppErrorCode::Io, format!("thumbnail encode failed: {e}")))?;
    Ok(format!("data:image/png;base64,{}", base64_encode(&buffer)))
}

/// Discards staged outputs after Clear results — files leave staging
/// immediately instead of only at app exit, and the ids become honestly
/// unsavable (`NotFound`) if a stale row somehow still references them.
#[tauri::command]
pub fn discard_staged(
    state: tauri::State<'_, StagingState>,
    staged_file_ids: Vec<String>,
) -> Result<usize, AppError> {
    state.discard(&staged_file_ids)
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

    let staged = state.resolve_staged(&staged_file_id)?;
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
    match std::fs::copy(&staged.path, &destination) {
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
        let staged = match state.resolve_staged(&staged_file_id) {
            Ok(p) => p,
            Err(e) => {
                results.push(SaveOutcome::SaveError {
                    staged_file_id: staged_file_id.clone(),
                    error: e,
                });
                continue;
            }
        };

        // Determine output name: keep the human-readable converted name
        // (e.g. `photo.png`) that staging produced, with _1, _2, … suffixes
        // on collision — never the opaque staged_file_id.
        let file_name = staged.file_name;
        let mut output_name = file_name.clone();
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
        match std::fs::copy(&staged.path, &destination) {
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
    fn base64_encoding_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn staging_register_resolve_discard_roundtrip() {
        let manager = StagingState::default();
        let batch = manager.new_batch().expect("new batch");
        std::fs::write(batch.join("photo.png"), b"fake png bytes").expect("write staged");

        manager
            .register("staged-0", &batch, "photo.png")
            .expect("register");

        let resolved = manager.resolve_staged("staged-0").expect("resolve");
        assert_eq!(resolved.file_name, "photo.png");
        assert_eq!(
            std::fs::read(&resolved.path).expect("read staged"),
            b"fake png bytes"
        );

        // Unknown ids report honest NotFound — they can never reference a
        // path outside staging because ids never join paths.
        let error = manager.resolve_staged("staged-1").expect_err("unknown id");
        assert_eq!(error.code, AppErrorCode::NotFound);

        // Discard removes the file and the batch directory.
        let discarded = manager.discard(&["staged-0".to_string()]).expect("discard");
        assert_eq!(discarded, 1);
        assert!(!batch.exists(), "empty batch directory must be removed");
        let error = manager
            .resolve_staged("staged-0")
            .expect_err("discarded id");
        assert_eq!(error.code, AppErrorCode::NotFound);

        // Discarding an unknown id is a no-op, still OK.
        let discarded = manager
            .discard(&["staged-1".to_string()])
            .expect("discard unknown");
        assert_eq!(discarded, 0);
    }

    #[test]
    fn staging_batches_are_isolated_so_same_names_never_collide() {
        let manager = StagingState::default();
        let batch_a = manager.new_batch().expect("batch a");
        let batch_b = manager.new_batch().expect("batch b");
        assert_ne!(batch_a, batch_b);

        std::fs::write(batch_a.join("photo.png"), b"from batch a").expect("write a");
        std::fs::write(batch_b.join("photo.png"), b"from batch b").expect("write b");
        manager
            .register("staged-a", &batch_a, "photo.png")
            .expect("register a");
        manager
            .register("staged-b", &batch_b, "photo.png")
            .expect("register b");

        // Same file name in both batches, but each id resolves to its own
        // batch directory — a second conversion can never overwrite the
        // first batch's staged copy.
        let resolved_a = manager.resolve_staged("staged-a").expect("resolve a");
        let resolved_b = manager.resolve_staged("staged-b").expect("resolve b");
        assert_ne!(resolved_a.path, resolved_b.path);
        assert_eq!(
            std::fs::read(&resolved_a.path).expect("read a"),
            b"from batch a"
        );
        assert_eq!(
            std::fs::read(&resolved_b.path).expect("read b"),
            b"from batch b"
        );
    }

    #[test]
    fn staging_rejects_traversal_style_file_names() {
        let manager = StagingState::default();
        let batch = manager.new_batch().expect("batch");
        for bad in ["..", "../evil.png", "sub\\dir.png", "", "."] {
            let error = manager
                .register("staged-x", &batch, bad)
                .expect_err("must reject");
            assert_eq!(error.code, AppErrorCode::InvalidInput);
        }
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
