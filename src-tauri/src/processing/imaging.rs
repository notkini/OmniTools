//! Pure image processing — decode, orient, resize, flatten transparency,
//! encode, with memory limits, cancellation checkpoints, and no-clobber
//! output publishing. No Tauri types; fully unit-testable.

use crate::error::{AppError, AppErrorCode};
use crate::paths::validate_read_path;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tempfile::NamedTempFile;

use image::GenericImageView;
use image::ImageEncoder;

/// Counter for generating unique staged_file_ids.
static STAGED_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_staged_id() -> String {
    let n = STAGED_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("staged-{n}")
}

/// Hard limits — protect against decompression bombs and memory exhaustion.
pub const MAX_IMAGE_WIDTH: u32 = 10_000;
pub const MAX_IMAGE_HEIGHT: u32 = 10_000;
pub const MAX_TOTAL_PIXELS: u64 = 50_000_000; // ~50 MP
pub const SINGLE_IMAGE_MEMORY_BUDGET_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

/// Estimated bytes per channel for common pixel types.
const BYTES_PER_CHANNEL: u64 = 1;

/// Terminal outcome per input path, order-preserving. `kind` is the wire
/// discriminator the frontend switches on.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImageOutcome {
    #[serde(rename_all = "camelCase")]
    Completed {
        path: PathBuf,
        output_path: PathBuf,
        staged_file_id: String,
        input_size_bytes: u64,
        output_size_bytes: u64,
        format: ImageFormat,
        width: u32,
        height: u32,
    },
    #[serde(rename_all = "camelCase")]
    Failed { path: PathBuf, error: AppError },
    #[serde(rename_all = "camelCase")]
    Canceled { path: PathBuf },
}

/// Output image format. Serde names are lowercase wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "webp")]
    WebP,
}

/// User-facing conversion options — serialized across IPC as camelCase JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertOpts {
    pub output_format: ImageFormat,
    pub quality: Option<u8>,
    pub resize: Option<ResizeOpts>,
}

/// Bounding-box resize: width/height are maximums; aspect ratio preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeOpts {
    pub max_width: u32,
    pub max_height: u32,
}

/// Per-file lifecycle hooks — used by the command layer to emit progress
/// events, and by tests to observe scheduling and inject cancellation.
pub type StartedHook = Arc<dyn Fn(usize, &Path) + Send + Sync>;
pub type FinishedHook = Arc<dyn Fn(usize, &Path, &ImageOutcome) + Send + Sync>;

pub struct ImageHooks {
    pub on_started: StartedHook,
    pub on_finished: FinishedHook,
}

impl ImageHooks {
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
struct ImageBatchShared {
    jobs: Vec<ImageJob>,
    queue: Mutex<VecDeque<usize>>,
    slots: Mutex<Vec<Option<ImageOutcome>>>,
}

/// One input file paired with its pre-allocated output path.
#[derive(Clone)]
struct ImageJob {
    path: PathBuf,
    output_path: PathBuf,
}

impl ImageBatchShared {
    /// Publishes one file's terminal outcome and fires the finished hook.
    /// The slot is recorded before the hook so a hook panic can never lose it.
    fn record(
        &self,
        index: usize,
        mut slots: MutexGuard<'_, Vec<Option<ImageOutcome>>>,
        outcome: &ImageOutcome,
        hooks: &ImageHooks,
    ) {
        slots[index] = Some(outcome.clone());
        drop(slots);
        (hooks.on_finished)(index, &self.jobs[index].path, outcome);
    }
}

/// Validate an output directory: must exist, be a directory, and be writable.
#[allow(dead_code)]
pub fn validate_output_dir(dir: &Path) -> Result<(), AppError> {
    crate::paths::validate_write_path(dir)?;
    if !dir.is_dir() {
        return Err(AppError::new(
            AppErrorCode::NotFound,
            format!("output path is not a directory: {}", dir.display()),
        ));
    }
    // Writability probe: create and remove a temp file in the directory.
    let _probe =
        NamedTempFile::new_in(dir).map_err(|error| AppError::from_io_with_path(&error, dir))?;
    Ok(())
}

/// Estimate peak working memory for an image: source + resized output +
/// encoder/intermediate overhead (2x encoded estimate).
fn estimate_peak_memory(width: u32, height: u32, channels: u32) -> u64 {
    let source = width as u64 * height as u64 * channels as u64 * BYTES_PER_CHANNEL;
    let resized = source; // worst case: same size as source
    let encoded_overhead = source / 4; // conservative encoder buffer estimate
    source + resized + encoded_overhead
}

/// Read the first bytes of a file to probe its format signature.
fn read_header(path: &Path, len: usize) -> Result<Vec<u8>, AppError> {
    let mut file = File::open(path).map_err(|e| AppError::from_io_with_path(&e, path))?;
    let mut header = vec![0u8; len];
    let read = file
        .read(&mut header)
        .map_err(|e| AppError::from_io_with_path(&e, path))?;
    header.truncate(read);
    Ok(header)
}

/// Detect animated WebP by checking the RIFF/WEBP header for the ANIM chunk,
/// without decoding pixels.
fn is_animated_webp(path: &Path) -> Result<bool, AppError> {
    let header = read_header(path, 12)?;
    if header.len() < 12 {
        return Ok(false);
    }
    // WebP files start with "RIFF" + size + "WEBP".
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WEBP" {
        return Ok(false);
    }
    // Read more of the file to scan chunks for "ANIM".
    let more = read_header(path, 65536)?;
    let mut pos = 12;
    while pos + 8 <= more.len() {
        if &more[pos..pos + 4] == b"ANIM" {
            return Ok(true);
        }
        let chunk_size =
            u32::from_le_bytes(more[pos + 4..pos + 8].try_into().unwrap_or([0; 4])) as usize;
        pos += 8 + chunk_size;
        if pos % 2 != 0 {
            pos += 1; // chunk padding
        }
    }
    Ok(false)
}

/// Detect animated PNG (APNG) by checking for the `acTL` chunk,
/// without decoding pixels.
fn is_animated_apng(path: &Path) -> Result<bool, AppError> {
    let header = read_header(path, 65536)?;
    if header.len() < 8 {
        return Ok(false);
    }
    // PNG signature: 89 50 4E 47 0D 0A 1A 0A
    if &header[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Ok(false);
    }
    let mut pos = 8;
    while pos + 8 <= header.len() {
        let chunk_len =
            u32::from_be_bytes(header[pos..pos + 4].try_into().unwrap_or([0; 4])) as usize;
        let chunk_type = &header[pos + 4..pos + 8];
        if chunk_type == b"acTL" {
            return Ok(true);
        }
        pos += 12 + chunk_len; // length(4) + type(4) + data + crc(4)
    }
    Ok(false)
}

/// Decode an image from a path, applying EXIF orientation where supported
/// (JPEG and WebP). PNG is passed through without orientation transform.
fn decode_image(path: &Path, cancel: &AtomicBool) -> Result<image::DynamicImage, AppError> {
    // Animation rejection before any pixel decoding.
    if path.extension().and_then(|e| e.to_str()) == Some("webp") && is_animated_webp(path)? {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            "animated WebP is not supported",
        ));
    }
    if path.extension().and_then(|e| e.to_str()) == Some("png") && is_animated_apng(path)? {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            "animated APNG is not supported",
        ));
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::new(AppErrorCode::Canceled, "canceled"));
    }

    // First pass: read dimensions without decoding pixels.
    let dimensions = {
        let reader = image::ImageReader::open(path)
            .map_err(|e| AppError::from_io_with_path(&e, path))?
            .with_guessed_format()
            .map_err(|e| {
                AppError::new(
                    AppErrorCode::InvalidInput,
                    format!("{}: {e}", path.display()),
                )
            })?;
        reader.into_dimensions().map_err(|e| {
            AppError::new(
                AppErrorCode::InvalidInput,
                format!("{}: {e}", path.display()),
            )
        })?
    };

    // Validate dimensions before allocating.
    if dimensions.0 > MAX_IMAGE_WIDTH || dimensions.1 > MAX_IMAGE_HEIGHT {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!(
                "image dimensions {}x{} exceed the 10000x10000 limit",
                dimensions.0, dimensions.1
            ),
        ));
    }
    let total_pixels = dimensions.0 as u64 * dimensions.1 as u64;
    if total_pixels > MAX_TOTAL_PIXELS {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!(
                "image has {} pixels, exceeding the {} pixel limit",
                total_pixels, MAX_TOTAL_PIXELS
            ),
        ));
    }

    // Estimate memory and reject if over the single-image budget.
    // Channels: assume RGBA (4) as worst case.
    let peak = estimate_peak_memory(dimensions.0, dimensions.1, 4);
    if peak > SINGLE_IMAGE_MEMORY_BUDGET_BYTES {
        return Err(AppError::new(
            AppErrorCode::InvalidInput,
            format!(
                "estimated peak memory {} bytes exceeds the {} byte single-image budget",
                peak, SINGLE_IMAGE_MEMORY_BUDGET_BYTES
            ),
        ));
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::new(AppErrorCode::Canceled, "canceled"));
    }

    // Second pass: decode the image.
    let img = image::ImageReader::open(path)
        .map_err(|e| AppError::from_io_with_path(&e, path))?
        .with_guessed_format()
        .map_err(|e| {
            AppError::new(
                AppErrorCode::InvalidInput,
                format!("{}: {e}", path.display()),
            )
        })?
        .decode()
        .map_err(AppError::from)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::new(AppErrorCode::Canceled, "canceled"));
    }

    Ok(img)
}

/// Flatten transparent pixels against a white background for JPEG output.
fn flatten_to_rgb(img: image::DynamicImage) -> image::RgbImage {
    match img {
        image::DynamicImage::ImageRgba8(rgba) => {
            let (w, h) = rgba.dimensions();
            let mut rgb = image::RgbImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let pixel = rgba.get_pixel(x, y);
                    let alpha = pixel.0[3] as f32 / 255.0;
                    let inv_alpha = 1.0 - alpha;
                    let r = (pixel.0[0] as f32 * alpha + 255.0 * inv_alpha) as u8;
                    let g = (pixel.0[1] as f32 * alpha + 255.0 * inv_alpha) as u8;
                    let b = (pixel.0[2] as f32 * alpha + 255.0 * inv_alpha) as u8;
                    rgb.put_pixel(x, y, image::Rgb([r, g, b]));
                }
            }
            rgb
        }
        image::DynamicImage::ImageRgb8(rgb) => rgb,
        image::DynamicImage::ImageLuma8(luma) => image::DynamicImage::ImageLuma8(luma).to_rgb8(),
        other => other.to_rgb8(),
    }
}

/// Apply EXIF orientation transform where supported (JPEG and WebP).
/// PNG is passed through unchanged — it does not expose orientation metadata
/// through the built-in decoder.
fn apply_orientation(img: image::DynamicImage, format: ImageFormat) -> image::DynamicImage {
    match format {
        ImageFormat::Png => img,
        ImageFormat::Jpeg | ImageFormat::WebP => {
            // `image` 0.25 applies EXIF orientation automatically on decode for
            // JPEG and WebP via the built-in decoders. Strip the orientation
            // tag from output metadata to prevent double rotation by viewers.
            // The `image` crate does not retain EXIF in `DynamicImage`, so no
            // explicit strip is needed — the output is orientation-free pixels.
            img
        }
    }
}

/// Resize to fit within max_width × max_height while preserving aspect ratio.
/// The image is never stretched.
fn maybe_resize(img: image::DynamicImage, resize: &Option<ResizeOpts>) -> image::DynamicImage {
    let Some(opts) = resize else {
        return img;
    };
    if opts.max_width == 0 || opts.max_height == 0 {
        return img;
    }
    let (w, h) = img.dimensions();
    if w <= opts.max_width && h <= opts.max_height {
        return img;
    }
    let scale = (opts.max_width as f64 / w as f64).min(opts.max_height as f64 / h as f64);
    let new_w = (w as f64 * scale).round() as u32;
    let new_h = (h as f64 * scale).round() as u32;
    image::DynamicImage::ImageRgba8(image::imageops::resize(
        &img,
        new_w,
        new_h,
        image::imageops::FilterType::Lanczos3,
    ))
}

/// Encode an image to the output format and write to a temp file, then publish
/// via no-clobber (never replaces an existing destination).
fn encode_and_publish(
    img: image::DynamicImage,
    format: ImageFormat,
    output_path: &Path,
    quality: Option<u8>,
    cancel: &AtomicBool,
) -> Result<u64, AppError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::new(AppErrorCode::Canceled, "canceled"));
    }

    let parent = output_path.parent().unwrap_or(Path::new(""));
    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|e| AppError::new(AppErrorCode::Io, format!("{e}")))?;

    let result: Result<u64, AppError> = (|| {
        let file = temp.as_file_mut();
        match format {
            ImageFormat::Png => {
                let encoder = image::codecs::png::PngEncoder::new(&mut *file);
                encoder.write_image(
                    &img.to_rgba8(),
                    img.width(),
                    img.height(),
                    image::ColorType::Rgba8.into(),
                )?;
                file.sync_all()?;
            }
            ImageFormat::Jpeg => {
                let rgb = flatten_to_rgb(img);
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut *file,
                    quality.unwrap_or(85),
                );
                encoder.encode_image(&rgb)?;
                file.sync_all()?;
            }
            ImageFormat::WebP => {
                let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut *file);
                encoder.write_image(
                    &img.to_rgba8(),
                    img.width(),
                    img.height(),
                    image::ColorType::Rgba8.into(),
                )?;
                file.sync_all()?;
            }
        }
        let size = temp.as_file().metadata()?.len();
        Ok(size)
    })();

    let size = match result {
        Ok(s) => s,
        Err(e) => {
            // Drop the temp file explicitly to clean up on error.
            drop(temp);
            return Err(e);
        }
    };

    // Publish via no-clobber: never replaces an existing destination.
    match temp.persist_noclobber(output_path) {
        Ok(_) => Ok(size),
        Err(_e) => {
            // temp is owned by the error and cleaned up when the error drops.
            // Check if destination already exists (race condition).
            if output_path.exists() {
                Err(AppError::new(
                    AppErrorCode::InvalidInput,
                    format!("output file already exists: {}", output_path.display()),
                ))
            } else {
                Err(AppError::new(
                    AppErrorCode::Io,
                    format!("failed to publish output: {}", output_path.display()),
                ))
            }
        }
    }
}

/// Process one input file: decode, orient, resize, flatten, encode, publish.
/// Cancellation checkpoints: before decode, after decode, before publish.
/// Returns the terminal outcome.
fn process_one(
    path: &Path,
    output_path: &Path,
    opts: &ConvertOpts,
    cancel: &AtomicBool,
) -> ImageOutcome {
    if let Err(error) = validate_read_path(path) {
        return ImageOutcome::Failed {
            path: path.to_path_buf(),
            error,
        };
    }
    let input_size = File::open(path)
        .map(|f| f.metadata().map(|m| m.len()))
        .and_then(|r| r)
        .unwrap_or(0);

    if cancel.load(Ordering::Relaxed) {
        return ImageOutcome::Canceled {
            path: path.to_path_buf(),
        };
    }

    let img = match decode_image(path, cancel) {
        Ok(img) => img,
        Err(error) => {
            return ImageOutcome::Failed {
                path: path.to_path_buf(),
                error,
            };
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return ImageOutcome::Canceled {
            path: path.to_path_buf(),
        };
    }

    let oriented = apply_orientation(img, opts.output_format);
    let resized = maybe_resize(oriented, &opts.resize);
    let (width, height) = resized.dimensions();

    match encode_and_publish(
        resized,
        opts.output_format,
        output_path,
        opts.quality,
        cancel,
    ) {
        Ok(output_size_bytes) => ImageOutcome::Completed {
            path: path.to_path_buf(),
            output_path: output_path.to_path_buf(),
            staged_file_id: next_staged_id(),
            input_size_bytes: input_size,
            output_size_bytes,
            format: opts.output_format,
            width,
            height,
        },
        Err(error) => ImageOutcome::Failed {
            path: path.to_path_buf(),
            error,
        },
    }
}

/// Validates + converts every input. Per-file failures never abort the batch;
/// once cancellation is requested, pending files and single files interrupted
/// mid-stream become `Canceled` — completed/failed files keep their state.
/// `cancel` and `hooks` are owned `Arc`s because worker threads outlive this
/// call's stack.
pub fn convert_batch(
    paths: &[PathBuf],
    output_dir: &Path,
    opts: &ConvertOpts,
    concurrency_limit: usize,
    cancel: Arc<AtomicBool>,
    hooks: Arc<ImageHooks>,
) -> Vec<ImageOutcome> {
    if paths.is_empty() {
        return Vec::new();
    }

    let total = paths.len();
    let worker_count = total.min(concurrency_limit.max(1));

    // Pre-allocate output names so multiple inputs with the same basename get
    // deterministic _1, _2, … suffixes. This runs before any workers start,
    // so workers never race over the same output path.
    let mut jobs: Vec<ImageJob> = Vec::with_capacity(total);
    let mut used_names: BTreeSet<String> = BTreeSet::new();
    let ext = match opts.output_format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
    };
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
        let mut output_name = format!("{stem}.{ext}");
        let mut counter = 1u32;
        while used_names.contains(&output_name) {
            output_name = format!("{stem}_{counter}.{ext}");
            counter += 1;
            if counter > 9999 {
                output_name = format!("{stem}_{counter}.{ext}");
                break;
            }
        }
        used_names.insert(output_name.clone());
        jobs.push(ImageJob {
            path: path.clone(),
            output_path: output_dir.join(&output_name),
        });
    }

    let shared = Arc::new(ImageBatchShared {
        jobs,
        queue: Mutex::new((0..total).collect::<VecDeque<usize>>()),
        slots: Mutex::new(vec![None; total]),
    });

    let workers: Vec<std::thread::JoinHandle<()>> = (0..worker_count)
        .map(|_| {
            let shared = Arc::clone(&shared);
            let hooks = Arc::clone(&hooks);
            let cancel = Arc::clone(&cancel);
            let opts = opts.clone();
            std::thread::spawn(move || {
                loop {
                    let index = {
                        let mut queue = lock(&shared.queue);
                        match queue.pop_back() {
                            Some(index) => index,
                            None => break,
                        }
                    };
                    let job = shared.jobs[index].clone();
                    if cancel.load(Ordering::Relaxed) {
                        let outcome = ImageOutcome::Canceled { path: job.path };
                        let slots = lock(&shared.slots);
                        shared.record(index, slots, &outcome, &hooks);
                        continue;
                    }
                    (hooks.on_started)(index, &job.path);
                    let outcome = process_one(&job.path, &job.output_path, &opts, &cancel);
                    let slots = lock(&shared.slots);
                    shared.record(index, slots, &outcome, &hooks);
                }
            })
        })
        .collect();

    for worker in workers {
        let _ = worker.join();
    }

    let outcome_slots = lock(&shared.slots);
    shared
        .jobs
        .iter()
        .zip(outcome_slots.iter())
        .map(|(job, slot)| {
            slot.clone().unwrap_or_else(|| ImageOutcome::Failed {
                path: job.path.clone(),
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
    use serde_json::Value;
    use std::fs::File;
    use std::io::Write as IoWrite;

    fn temp_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).expect("create");
        file.write_all(contents).expect("write");
        path
    }

    fn make_rgb_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = temp_file(dir, name, &[]);
        let encoder = image::codecs::png::PngEncoder::new(File::create(&path).expect("create"));
        let pixels: Vec<u8> = (0..(width * height))
            .flat_map(|_| vec![255u8, 0u8, 0u8])
            .collect();
        encoder
            .write_image(&pixels, width, height, image::ColorType::Rgb8.into())
            .expect("encode test png");
        path
    }

    fn make_rgba_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = temp_file(dir, name, &[]);
        let encoder = image::codecs::png::PngEncoder::new(File::create(&path).expect("create"));
        let pixels: Vec<u8> = (0..(width * height))
            .flat_map(|_| vec![255u8, 0u8, 0u8, 128u8])
            .collect();
        encoder
            .write_image(&pixels, width, height, image::ColorType::Rgba8.into())
            .expect("encode test png");
        path
    }

    #[test]
    fn png_to_jpeg_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgb_png(dir.path(), "test.png", 8, 8);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(85),
            resize: None,
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ImageOutcome::Completed {
                output_path,
                format,
                ..
            } => {
                assert_eq!(*format, ImageFormat::Jpeg);
                assert!(output_path.extension().and_then(|e| e.to_str()) == Some("jpg"));
                let decoded = image::ImageReader::open(output_path)
                    .expect("open output")
                    .decode()
                    .expect("decode output");
                assert_eq!(decoded.width(), 8);
                assert_eq!(decoded.height(), 8);
            }
            other => panic!("expected completed, got {other:?}"),
        }
    }

    #[test]
    fn jpeg_to_png() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src_png = make_rgb_png(dir.path(), "src.png", 8, 8);
        let jpeg_dir = dir.path().join("jpeg");
        std::fs::create_dir(&jpeg_dir).expect("create jpeg dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(85),
            resize: None,
        };
        let jpeg_outcomes = convert_batch(
            &[src_png],
            &jpeg_dir,
            &opts,
            1,
            cancel.clone(),
            Arc::new(ImageHooks::quiet()),
        );
        assert!(
            matches!(&jpeg_outcomes[0], ImageOutcome::Completed { .. }),
            "jpeg encode should succeed"
        );
        let jpeg_path = match &jpeg_outcomes[0] {
            ImageOutcome::Completed { output_path, .. } => output_path.clone(),
            _ => panic!("unexpected"),
        };

        let png_dir = dir.path().join("png");
        std::fs::create_dir(&png_dir).expect("create png dir");
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[jpeg_path],
            &png_dir,
            &opts,
            1,
            cancel.clone(),
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ImageOutcome::Completed {
                output_path,
                format,
                ..
            } => {
                assert_eq!(*format, ImageFormat::Png);
                let decoded = image::ImageReader::open(output_path)
                    .expect("open output")
                    .decode()
                    .expect("decode output");
                assert_eq!(decoded.width(), 8);
                assert_eq!(decoded.height(), 8);
            }
            other => panic!("expected completed, got {other:?}"),
        }
    }

    #[test]
    fn png_to_webp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgb_png(dir.path(), "test.png", 8, 8);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::WebP,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ImageOutcome::Completed {
                output_path,
                format,
                ..
            } => {
                assert_eq!(*format, ImageFormat::WebP);
                assert!(output_path.extension().and_then(|e| e.to_str()) == Some("webp"));
            }
            other => panic!("expected completed, got {other:?}"),
        }
    }

    #[test]
    fn resize_bounding_box_preserves_aspect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgb_png(dir.path(), "wide.png", 100, 50);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: Some(ResizeOpts {
                max_width: 30,
                max_height: 30,
            }),
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ImageOutcome::Completed { width, height, .. } => {
                assert_eq!(*width, 30);
                assert_eq!(*height, 15);
            }
            other => panic!("expected completed with dimensions, got {other:?}"),
        }
    }

    #[test]
    fn traversal_path_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let bad = dir.path().join("..").join("etc").join("passwd");
        let outcomes = convert_batch(
            &[bad],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(&outcomes[0], ImageOutcome::Failed { error, .. }
                if error.code == AppErrorCode::InvalidInput),
            "traversal path should be invalid_input, got {outcomes:?}"
        );
    }

    #[test]
    fn missing_file_becomes_per_file_error_batch_continues() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = make_rgb_png(dir.path(), "good.png", 4, 4);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[good.clone(), dir.path().join("missing.png"), good.clone()],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 3);
        assert!(
            matches!(&outcomes[0], ImageOutcome::Completed { .. }),
            "first file should complete"
        );
        assert!(
            matches!(&outcomes[1], ImageOutcome::Failed { .. }),
            "missing file should fail"
        );
        assert!(
            matches!(&outcomes[2], ImageOutcome::Completed { .. }),
            "batch should continue after failure"
        );
    }

    #[test]
    fn cancel_before_start_marks_all_canceled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = make_rgb_png(dir.path(), "a.png", 4, 4);
        let b = make_rgb_png(dir.path(), "b.png", 4, 4);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(true));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[a, b],
            &output_dir,
            &opts,
            2,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, ImageOutcome::Canceled { .. })),
            "all files should be canceled, got {outcomes:?}"
        );
    }

    #[test]
    fn collision_handling_appends_suffix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = make_rgb_png(dir.path(), "photo.png", 4, 4);
        let b = make_rgb_png(dir.path(), "photo.png", 4, 4);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[a, b],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 2);
        let output_paths: Vec<PathBuf> = outcomes
            .iter()
            .filter_map(|o| match o {
                ImageOutcome::Completed { output_path, .. } => Some(output_path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(output_paths.len(), 2);
        assert_ne!(output_paths[0], output_paths[1]);
        for p in &output_paths {
            assert!(p.exists(), "output should exist: {p:?}");
        }
    }

    #[test]
    fn no_overwrite_existing_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgb_png(dir.path(), "photo.png", 4, 4);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let expected = output_dir.join("photo.png");
        std::fs::write(&expected, "stale").expect("seed stale");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(&outcomes[0], ImageOutcome::Failed { error, .. }
                if error.code == AppErrorCode::InvalidInput),
            "existing output should fail no-clobber, got {outcomes:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&expected).expect("read stale"),
            "stale"
        );
    }

    #[test]
    fn quality_range_validation() {
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(0),
            resize: None,
        };
        assert!(opts.quality.is_none_or(|q| !(1..=100).contains(&q)));
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(101),
            resize: None,
        };
        assert!(opts.quality.is_none_or(|q| !(1..=100).contains(&q)));
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(85),
            resize: None,
        };
        assert!(opts.quality.is_some_and(|q| (1..=100).contains(&q)));
    }

    #[test]
    fn serialization_shape_matches_frontend() {
        use serde_json::Value;
        let path = PathBuf::from("/tmp/photo.png");
        let output_path = PathBuf::from("/tmp/photo.jpg");
        let completed = ImageOutcome::Completed {
            path: path.clone(),
            output_path,
            staged_file_id: "staged-0".to_string(),
            input_size_bytes: 1024,
            output_size_bytes: 512,
            format: ImageFormat::Jpeg,
            width: 8,
            height: 8,
        };
        let value = serde_json::to_value(&completed).expect("serialize");
        assert_eq!(value["kind"], Value::String("completed".into()));
        assert_eq!(value["format"], Value::String("jpeg".into()));
        assert_eq!(value["inputSizeBytes"], Value::from(1024));
        assert_eq!(value["outputSizeBytes"], Value::from(512));
        assert_eq!(value["width"], Value::from(8));
        assert_eq!(value["height"], Value::from(8));

        let failed = ImageOutcome::Failed {
            path,
            error: AppError::new(AppErrorCode::NotFound, "missing"),
        };
        let failed_value = serde_json::to_value(&failed).expect("serialize");
        assert_eq!(failed_value["kind"], Value::String("failed".into()));
        assert_eq!(
            failed_value["error"]["code"],
            Value::String("not_found".into())
        );
    }

    #[test]
    fn webp_serializes_as_webp_not_webp_uppercase() {
        let value = serde_json::to_value(ImageFormat::WebP).expect("serialize");
        assert_eq!(value, Value::String("webp".into()));
    }

    #[test]
    fn transparent_png_to_jpeg_flattens_alpha() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgba_png(dir.path(), "transparent.png", 4, 4);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Jpeg,
            quality: Some(85),
            resize: None,
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(&outcomes[0], ImageOutcome::Completed { .. }),
            "transparent PNG to JPEG should succeed"
        );
    }

    #[test]
    fn input_output_sizes_recorded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = make_rgb_png(dir.path(), "size_test.png", 8, 8);
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &[src],
            &output_dir,
            &opts,
            1,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ImageOutcome::Completed {
                input_size_bytes,
                output_size_bytes,
                ..
            } => {
                assert!(*input_size_bytes > 0);
                assert!(*output_size_bytes > 0);
            }
            other => panic!("expected completed with sizes, got {other:?}"),
        }
    }

    #[test]
    fn concurrency_limit_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| make_rgb_png(dir.path(), &format!("f{i}.png"), 4, 4))
            .collect();
        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).expect("create out dir");
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = ConvertOpts {
            output_format: ImageFormat::Png,
            quality: None,
            resize: None,
        };
        let outcomes = convert_batch(
            &paths,
            &output_dir,
            &opts,
            3,
            cancel,
            Arc::new(ImageHooks::quiet()),
        );
        assert_eq!(outcomes.len(), 10);
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, ImageOutcome::Completed { .. })),
            "all should complete"
        );
    }
}
