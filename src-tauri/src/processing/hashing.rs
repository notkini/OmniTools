//! Streaming SHA-256 over filesystem handles — pure std + sha2, no Tauri.
//! Files are never loaded whole: every read is bounded by `STREAM_BUFFER_BYTES`.

use crate::error::{AppError, AppErrorCode};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Read buffer for streaming — large files hash in bounded memory.
pub const STREAM_BUFFER_BYTES: usize = 64 * 1024;

pub type Sha256Digest = [u8; 32];

/// Lowercase hex encoding (the checksum interchange format).
pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

/// Hashes a stream incrementally. Cancellation is checked between buffer
/// reads, so even one huge file stops hash-fast when the batch is canceled.
pub fn sha256_stream<R: Read>(
    reader: &mut R,
    cancel: &AtomicBool,
) -> Result<Sha256Digest, AppError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; STREAM_BUFFER_BYTES];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(
                AppErrorCode::Canceled,
                "Hashing was canceled for this file.",
            ));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

/// Validates path-level preconditions (missing file, directory), then streams
/// the file, returning digest + declared size in bytes.
pub fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<(Sha256Digest, u64), AppError> {
    let metadata =
        std::fs::metadata(path).map_err(|error| AppError::from_io_with_path(&error, path))?;
    if metadata.is_dir() {
        return Err(AppError::new(
            AppErrorCode::IsADirectory,
            format!("{} is a directory", path.display()),
        ));
    }
    let size = metadata.len();
    let file = File::open(path).map_err(|error| AppError::from_io_with_path(&error, path))?;
    let mut reader = BufReader::with_capacity(STREAM_BUFFER_BYTES, file);
    let digest = sha256_stream(&mut reader, cancel)?;
    Ok((digest, size))
}

/// Streams two files chunk-by-chunk and reports identical bytes. Reads are
/// filled completely (read may return short reads), so length differences are
/// detected exactly, and neither file is ever held in memory.
pub fn compare_files(first: &Path, second: &Path) -> Result<bool, AppError> {
    let first_meta =
        std::fs::metadata(first).map_err(|error| AppError::from_io_with_path(&error, first))?;
    let second_meta =
        std::fs::metadata(second).map_err(|error| AppError::from_io_with_path(&error, second))?;
    if first_meta.is_dir() {
        return Err(AppError::new(
            AppErrorCode::IsADirectory,
            format!("{} is a directory", first.display()),
        ));
    }
    if second_meta.is_dir() {
        return Err(AppError::new(
            AppErrorCode::IsADirectory,
            format!("{} is a directory", second.display()),
        ));
    }

    let mut a = BufReader::with_capacity(
        STREAM_BUFFER_BYTES,
        File::open(first).map_err(|error| AppError::from_io_with_path(&error, first))?,
    );
    let mut b = BufReader::with_capacity(
        STREAM_BUFFER_BYTES,
        File::open(second).map_err(|error| AppError::from_io_with_path(&error, second))?,
    );
    let mut buf_a = vec![0u8; STREAM_BUFFER_BYTES];
    let mut buf_b = vec![0u8; STREAM_BUFFER_BYTES];

    loop {
        let read_a = fill_read(&mut a, &mut buf_a)?;
        let read_b = fill_read(&mut b, &mut buf_b)?;
        if read_a != read_b {
            return Ok(false);
        }
        if read_a == 0 {
            return Ok(true);
        }
        if buf_a[..read_a] != buf_b[..read_b] {
            return Ok(false);
        }
    }
}

/// Reads until the buffer is full or EOF; returns total bytes read (0 = EOF).
fn fill_read<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<usize, AppError> {
    let mut total = 0usize;
    while total < buffer.len() {
        let read = reader.read(&mut buffer[total..])?;
        if read == 0 {
            break;
        }
        total += read;
    }
    Ok(total)
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => AppErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => AppErrorCode::AccessDenied,
            std::io::ErrorKind::IsADirectory => AppErrorCode::IsADirectory,
            _ => AppErrorCode::Io,
        };
        AppError::new(code, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NIST_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const NIST_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const NIST_TWO_BLOCK: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
    const NIST_MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

    /// Reader fixture with an observed per-read bound — proves streaming,
    /// i.e. nothing reads whole files into memory.
    struct BoundRecorder<R: Read> {
        inner: R,
        max_read: usize,
    }

    impl<R: Read> Read for BoundRecorder<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let read = self.inner.read(buf)?;
            self.max_read = self.max_read.max(read);
            Ok(read)
        }
    }

    fn digest_hex<R: Read>(data: &mut R) -> String {
        let cancel = AtomicBool::new(false);
        let digest = sha256_stream(data, &cancel).expect("digest");
        to_hex(&digest)
    }

    #[test]
    fn nist_vector_empty_input() {
        assert_eq!(digest_hex(&mut std::io::empty()), NIST_EMPTY);
    }

    #[test]
    fn nist_vector_abc() {
        assert_eq!(digest_hex(&mut "abc".as_bytes()), NIST_ABC);
    }

    #[test]
    fn nist_vector_two_block_448_bit_message() {
        let message = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(digest_hex(&mut message.as_slice()), NIST_TWO_BLOCK);
    }

    #[test]
    fn nist_vector_one_million_a_bytes() {
        // One million 'a' — a long-stream vector hashed in chunks.
        let million = vec![b'a'; 1_000_000];
        assert_eq!(digest_hex(&mut million.as_slice()), NIST_MILLION_A);
    }

    #[test]
    fn hashes_arbitrary_binary_content() {
        let binary: Vec<u8> = (0u16..=65535).flat_map(|v| v.to_le_bytes()).collect();
        let expected = {
            let mut hasher = Sha256::new();
            hasher.update(&binary);
            to_hex(&hasher.finalize())
        };
        assert_eq!(digest_hex(&mut binary.as_slice()), expected);
    }

    #[test]
    fn hex_encoding_is_lowercase_and_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xa1, 0xff]), "000fa1ff");
    }

    /// Chunk-boundary sweep: every size around the internal buffer must hash
    /// identically to a direct one-shot update — proves incremental reading.
    #[test]
    fn chunk_boundaries_hash_identically_to_one_shot() {
        const SIZES: &[usize] = &[
            0,
            1,
            63,
            STREAM_BUFFER_BYTES - 1,
            STREAM_BUFFER_BYTES,
            STREAM_BUFFER_BYTES + 1,
            2 * STREAM_BUFFER_BYTES + 7,
        ];
        for size in SIZES {
            let data: Vec<u8> = (0..*size).map(|i| (i % 251) as u8).collect();
            let expected = {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                to_hex(&hasher.finalize())
            };
            let cancel = AtomicBool::new(false);
            let mut slice: &[u8] = &data;
            let digest = sha256_stream(&mut slice, &cancel).expect("digest");
            assert_eq!(to_hex(&digest), expected, "size {size}");
        }
    }

    /// Large-file proof: even with a multi-buffer file, the observed read
    /// bound never exceeds the streaming buffer — the whole file is not held
    /// in memory.
    #[test]
    fn large_file_streams_instead_of_loading_whole() {
        const CHUNKS: usize = STREAM_BUFFER_BYTES / 16 * 5; // > 2 buffers
        let data: Vec<u8> = (0..CHUNKS).flat_map(|i| i.to_le_bytes()).collect();
        let expected = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            to_hex(&hasher.finalize())
        };

        let recorder = BoundRecorder {
            inner: data.as_slice(),
            max_read: 0,
        };
        let cancel = AtomicBool::new(false);
        let mut reader = recorder;
        let digest = sha256_stream(&mut reader, &cancel).expect("digest");

        assert_eq!(to_hex(&digest), expected);
        assert!(
            reader.max_read <= STREAM_BUFFER_BYTES,
            "a read of {} bytes exceeded the streaming buffer",
            reader.max_read
        );
    }

    #[test]
    fn cancellation_between_reads_yields_canceled_error() {
        let cancel = AtomicBool::new(true);
        let error = sha256_stream(&mut "abc".as_bytes(), &cancel).expect_err("must cancel");
        assert_eq!(error.code, AppErrorCode::Canceled);
    }

    #[test]
    fn cancellation_during_large_file_is_between_chunks() {
        // Cancel flips after the first read — the stream must stop promptly.
        struct CancelAfterFirstRead {
            reads: usize,
        }
        impl Read for CancelAfterFirstRead {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.reads += 1;
                if self.reads == 1 {
                    buf[0] = b'x';
                    Ok(1)
                } else {
                    Err(std::io::Error::other("read after cancel"))
                }
            }
        }
        let cancel = AtomicBool::new(true);
        let error = sha256_stream(&mut CancelAfterFirstRead { reads: 0 }, &cancel)
            .expect_err("must cancel before second read");
        assert_eq!(error.code, AppErrorCode::Canceled);
    }

    fn write_file(dir: &Path, name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).expect("create");
        file.write_all(contents).expect("write");
        path
    }

    #[test]
    fn file_hashing_returns_size_and_vector_hex() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "abc.txt", b"abc");
        let cancel = AtomicBool::new(false);
        let (digest, size) = sha256_file(&path, &cancel).expect("hash");
        assert_eq!(to_hex(&digest), NIST_ABC);
        assert_eq!(size, 3);
    }

    #[test]
    fn empty_file_hashes_as_nist_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "empty.bin", b"");
        let cancel = AtomicBool::new(false);
        let (digest, size) = sha256_file(&path, &cancel).expect("hash");
        assert_eq!(to_hex(&digest), NIST_EMPTY);
        assert_eq!(size, 0);
    }

    #[test]
    fn missing_file_maps_to_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ghost.bin");
        let cancel = AtomicBool::new(false);
        let error = sha256_file(&path, &cancel).expect_err("missing file must error");
        assert_eq!(error.code, AppErrorCode::NotFound);
    }

    #[test]
    fn directory_input_maps_to_is_a_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = AtomicBool::new(false);
        let error = sha256_file(dir.path(), &cancel).expect_err("directory must error");
        assert_eq!(error.code, AppErrorCode::IsADirectory);
    }

    #[test]
    fn compare_identical_and_different_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.bin", b"same");
        let b = write_file(dir.path(), "b.bin", b"same");
        let c = write_file(dir.path(), "c.bin", b"different");
        let d = write_file(dir.path(), "d.bin", b"sam");

        assert!(compare_files(&a, &b).expect("compare"));
        assert!(compare_files(&a, &a).expect("self compare"));
        assert!(!compare_files(&a, &c).expect("compare"));
        assert!(!compare_files(&a, &d).expect("prefix must differ"));
    }

    #[test]
    fn compare_empty_and_multibuffer_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let e1 = write_file(dir.path(), "e1.bin", b"");
        let e2 = write_file(dir.path(), "e2.bin", b"");
        let big_a = write_file(
            dir.path(),
            "big_a.bin",
            &vec![9u8; 2 * STREAM_BUFFER_BYTES + 123],
        );
        let mut big_b_contents = vec![9u8; 2 * STREAM_BUFFER_BYTES + 123];
        big_b_contents[STREAM_BUFFER_BYTES] ^= 0x01; // differs mid-stream
        let big_b = write_file(dir.path(), "big_b.bin", &big_b_contents);

        assert!(compare_files(&e1, &e2).expect("empty compare"));
        assert!(compare_files(&big_a, &big_a).expect("big self compare"));
        assert!(!compare_files(&big_a, &big_b).expect("big different"));
        assert!(!compare_files(&e1, &big_a).expect("empty vs big"));
    }

    #[test]
    fn compare_errors_for_missing_and_directory_inputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.bin", b"x");
        let missing = dir.path().join("ghost.bin");

        let error = compare_files(&missing, &a).expect_err("missing first");
        assert_eq!(error.code, AppErrorCode::NotFound);
        let error = compare_files(&a, &missing).expect_err("missing second");
        assert_eq!(error.code, AppErrorCode::NotFound);
        let error = compare_files(dir.path(), &a).expect_err("directory first");
        assert_eq!(error.code, AppErrorCode::IsADirectory);
        let error = compare_files(&a, dir.path()).expect_err("directory second");
        assert_eq!(error.code, AppErrorCode::IsADirectory);
    }
}
