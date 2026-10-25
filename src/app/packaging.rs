use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::error::AppError;
use super::types::{EventSink, PlenumEvent, SessionControl, TransferEvent};

/// RAII guard that automatically removes a temporary archive file when dropped.
#[derive(Debug)]
pub struct TempArchiveGuard {
    path: PathBuf,
    active: bool,
}

impl TempArchiveGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disables automatic deletion if the caller wants to retain the file.
    #[allow(dead_code)]
    pub fn defuse(&mut self) {
        self.active = false;
    }
}

impl Drop for TempArchiveGuard {
    fn drop(&mut self) {
        if self.active && self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Represents the prepared payload ready for network transmission.
#[derive(Debug)]
pub enum PreparedPayload {
    /// Single regular file directly streamed without packaging.
    Direct {
        path: PathBuf,
        file_name: String,
        file_size: u64,
    },
    /// Multiple files or directory bundled into a temporary ZIP archive.
    Archived {
        guard: TempArchiveGuard,
        display_name: String,
        file_size: u64,
    },
}

impl PreparedPayload {
    pub fn file_path(&self) -> &Path {
        match self {
            Self::Direct { path, .. } => path,
            Self::Archived { guard, .. } => guard.path(),
        }
    }

    pub fn file_name(&self) -> &str {
        match self {
            Self::Direct { file_name, .. } => file_name,
            Self::Archived { display_name, .. } => display_name,
        }
    }

    pub fn file_size(&self) -> u64 {
        match self {
            Self::Direct { file_size, .. } => *file_size,
            Self::Archived { file_size, .. } => *file_size,
        }
    }
}

/// Determines whether a file is likely pre-compressed based on its extension.
/// For pre-compressed formats, we use `Stored` (no compression) to achieve disk-speed transfers.
pub fn is_precompressed(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "zip"
            | "gz"
            | "tar"
            | "tgz"
            | "bz2"
            | "xz"
            | "7z"
            | "rar"
            | "zst"
            | "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "webp"
            | "avif"
            | "heic"
            | "mp4"
            | "mkv"
            | "avi"
            | "mov"
            | "webm"
            | "mp3"
            | "aac"
            | "ogg"
            | "flac"
            | "m4a"
            | "pdf"
            | "docx"
            | "xlsx"
            | "pptx"
            | "epub"
            | "apk"
            | "ipa"
            | "iso"
            | "dmg"
    )
}

/// Generates a clean, user-friendly display name for the ZIP archive.
pub fn generate_archive_name(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        return format!("plenum_archive_{ts}.zip");
    }

    if paths.len() == 1 && paths[0].is_dir() {
        let dir_name = paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if !dir_name.is_empty() {
            return format!("{dir_name}.zip");
        }
    }

    let first = &paths[0];
    let stem = first
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    if paths.len() > 1 && !stem.is_empty() {
        format!("{stem}_and_{}_more.zip", paths.len() - 1)
    } else if !stem.is_empty() {
        format!("{stem}.zip")
    } else {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("plenum_archive_{ts}.zip")
    }
}

/// Disambiguates an entry path inside the ZIP archive if an entry with the same name already exists.
fn disambiguate_entry_name(name: &str, existing: &HashSet<String>) -> String {
    if !existing.contains(name) {
        return name.to_string();
    }

    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().map(|e| e.to_string_lossy());
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    let mut counter = 1usize;
    loop {
        let candidate_file = if let Some(ref ext_str) = ext {
            format!("{stem} ({counter}).{ext_str}")
        } else {
            format!("{stem} ({counter})")
        };

        let candidate = if parent.as_os_str().is_empty() {
            candidate_file
        } else {
            let mut p = parent.to_path_buf();
            p.push(candidate_file);
            p.to_slash_lossy()
        };

        if !existing.contains(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Normalizes path to forward slashes for ZIP format entries.
trait PathToZipSlash {
    fn to_slash_lossy(&self) -> String;
}

impl PathToZipSlash for Path {
    fn to_slash_lossy(&self) -> String {
        self.to_string_lossy().replace('\\', "/")
    }
}

struct PlannedEntry {
    disk_path: PathBuf,
    zip_path: String,
    is_dir: bool,
}

/// Prepares the payload for transfer.
/// - If `paths` contains exactly one regular file, packaging is bypassed completely.
/// - If `paths` contains multiple files or any directory, items are packaged into a ZIP archive.
pub fn prepare_payload<S: EventSink>(
    paths: &[PathBuf],
    control: &SessionControl,
    sink: &mut S,
) -> Result<PreparedPayload, AppError> {
    if paths.is_empty() {
        return Err(AppError::InvalidRequest("No files specified to send".into()));
    }

    // Verify all specified paths exist before doing any work
    for path in paths {
        if !path.exists() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File or directory not found: {}", path.display()),
            )));
        }
    }

    // Single regular file bypass (zero-overhead)
    if paths.len() == 1 && paths[0].is_file() {
        let path = paths[0].clone();
        let metadata = std::fs::metadata(&path)?;
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        return Ok(PreparedPayload::Direct {
            path,
            file_name,
            file_size: metadata.len(),
        });
    }

    // Multi-file or directory packaging
    let mut planned_entries: Vec<PlannedEntry> = Vec::new();
    let mut existing_zip_names: HashSet<String> = HashSet::new();

    for path in paths {
        if control.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        if path.is_file() {
            let raw_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let clean_name = disambiguate_entry_name(&raw_name, &existing_zip_names);
            existing_zip_names.insert(clean_name.clone());
            planned_entries.push(PlannedEntry {
                disk_path: path.clone(),
                zip_path: clean_name,
                is_dir: false,
            });
        } else if path.is_dir() {
            let dir_base_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dir_prefix = if dir_base_name.is_empty() {
                "folder".to_string()
            } else {
                dir_base_name
            };

            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if control.is_cancelled() {
                    return Err(AppError::Cancelled);
                }

                let entry_path = entry.path();
                let relative = match entry_path.strip_prefix(path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };

                let relative_slash = relative.to_slash_lossy();
                let zip_entry_path = if relative_slash.is_empty() {
                    // Root of this directory
                    format!("{dir_prefix}/")
                } else if entry.file_type().is_dir() {
                    format!("{dir_prefix}/{relative_slash}/")
                } else {
                    format!("{dir_prefix}/{relative_slash}")
                };

                let final_zip_path = disambiguate_entry_name(&zip_entry_path, &existing_zip_names);
                existing_zip_names.insert(final_zip_path.clone());

                let is_dir = entry.file_type().is_dir();
                planned_entries.push(PlannedEntry {
                    disk_path: entry_path.to_path_buf(),
                    zip_path: final_zip_path,
                    is_dir,
                });
            }
        }
    }

    let total_items = planned_entries.len();
    let display_name = generate_archive_name(paths);

    // Create temporary file
    let temp_dir = std::env::temp_dir();
    let rand_id = rand::random::<u32>();
    let temp_file_path = temp_dir.join(format!("plenum_tmp_{rand_id}_{display_name}"));
    let guard = TempArchiveGuard::new(temp_file_path.clone());

    let file = File::create(&temp_file_path)?;
    let buf_writer = BufWriter::with_capacity(1024 * 1024, file);
    let mut zip = ZipWriter::new(buf_writer);

    let mut bytes_packaged = 0u64;
    let mut chunk_buf = vec![0u8; 128 * 1024];

    for (idx, entry) in planned_entries.into_iter().enumerate() {
        if control.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        sink.emit(PlenumEvent::Transfer(TransferEvent::Packaging {
            current_item: idx + 1,
            total_items,
            current_name: entry.zip_path.clone(),
            bytes_packaged,
        }));

        if entry.is_dir {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(0o755);
            zip.add_directory(&entry.zip_path, options)
                .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
        } else {
            let ext = Path::new(&entry.zip_path)
                .extension()
                .map(|e| e.to_string_lossy())
                .unwrap_or_default();

            let method = if is_precompressed(&ext) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };

            let options = SimpleFileOptions::default()
                .compression_method(method)
                .unix_permissions(0o644);

            zip.start_file(&entry.zip_path, options)
                .map_err(|e| AppError::Io(std::io::Error::other(e)))?;

            let mut input_file = BufReader::with_capacity(128 * 1024, File::open(&entry.disk_path)?);
            loop {
                if control.is_cancelled() {
                    return Err(AppError::Cancelled);
                }

                let n = input_file.read(&mut chunk_buf)?;
                if n == 0 {
                    break;
                }
                zip.write_all(&chunk_buf[..n])?;
                bytes_packaged = bytes_packaged.saturating_add(n as u64);
            }
        }
    }

    let mut buf_writer = zip.finish().map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    buf_writer.flush()?;
    drop(buf_writer);

    let final_size = std::fs::metadata(&temp_file_path)?.len();

    sink.emit(PlenumEvent::Transfer(TransferEvent::Packaging {
        current_item: total_items,
        total_items,
        current_name: display_name.clone(),
        bytes_packaged: final_size,
    }));

    Ok(PreparedPayload::Archived {
        guard,
        display_name,
        file_size: final_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct DummySink;
    impl EventSink for DummySink {
        fn emit(&mut self, _event: PlenumEvent) {}
    }

    #[test]
    fn test_single_file_bypass() {
        let temp_dir = std::env::temp_dir().join(format!("plenum_test_bypass_{}", rand::random::<u32>()));
        let _ = fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("hello.txt");
        fs::write(&test_file, b"Hello world").unwrap();

        let control = SessionControl::new();
        let mut sink = DummySink;

        let payload = prepare_payload(&[test_file.clone()], &control, &mut sink).expect("prepare payload");
        match payload {
            PreparedPayload::Direct { path, file_name, file_size } => {
                assert_eq!(path, test_file);
                assert_eq!(file_name, "hello.txt");
                assert_eq!(file_size, 11);
            }
            PreparedPayload::Archived { .. } => panic!("Expected Direct payload for single file"),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_multi_file_packaging_and_cleanup() {
        let temp_dir = std::env::temp_dir().join(format!("plenum_test_multi_{}", rand::random::<u32>()));
        let _ = fs::create_dir_all(&temp_dir);
        let file1 = temp_dir.join("file1.txt");
        let file2 = temp_dir.join("file2.txt");
        fs::write(&file1, b"File 1 content").unwrap();
        fs::write(&file2, b"File 2 content").unwrap();

        let control = SessionControl::new();
        let mut sink = DummySink;

        let payload = prepare_payload(&[file1.clone(), file2.clone()], &control, &mut sink).expect("prepare multi payload");
        let archive_path = match payload {
            PreparedPayload::Archived { ref guard, ref display_name, file_size } => {
                assert!(display_name.contains("file1_and_1_more.zip"));
                assert!(file_size > 0);
                assert!(guard.path().exists());
                guard.path().to_path_buf()
            }
            PreparedPayload::Direct { .. } => panic!("Expected Archived payload for multiple files"),
        };

        // Validate ZIP contents
        let zip_file = File::open(&archive_path).expect("open zip");
        let mut zip = zip::ZipArchive::new(zip_file).expect("read zip archive");
        assert_eq!(zip.len(), 2);
        let file_names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
        assert!(file_names.contains(&"file1.txt".to_string()));
        assert!(file_names.contains(&"file2.txt".to_string()));

        // Drop payload to trigger RAII TempArchiveGuard deletion
        drop(payload);
        assert!(!archive_path.exists(), "Temporary archive must be deleted on drop");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_folder_packaging() {
        let temp_dir = std::env::temp_dir().join(format!("plenum_test_dir_{}", rand::random::<u32>()));
        let folder = temp_dir.join("my_project");
        let sub = folder.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(folder.join("root.txt"), b"root content").unwrap();
        fs::write(sub.join("nested.txt"), b"nested content").unwrap();

        let control = SessionControl::new();
        let mut sink = DummySink;

        let payload = prepare_payload(&[folder.clone()], &control, &mut sink).expect("prepare folder payload");
        let archive_path = match payload {
            PreparedPayload::Archived { ref guard, ref display_name, file_size } => {
                assert_eq!(display_name, "my_project.zip");
                assert!(file_size > 0);
                assert!(guard.path().exists());
                guard.path().to_path_buf()
            }
            PreparedPayload::Direct { .. } => panic!("Expected Archived payload for folder"),
        };

        let zip_file = File::open(&archive_path).expect("open zip");
        let mut zip = zip::ZipArchive::new(zip_file).expect("read zip archive");
        let file_names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
        assert!(file_names.iter().any(|n| n.contains("my_project/root.txt")));
        assert!(file_names.iter().any(|n| n.contains("my_project/sub/nested.txt")));

        drop(payload);
        assert!(!archive_path.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_cancellation_during_packaging() {
        let temp_dir = std::env::temp_dir().join(format!("plenum_test_cancel_{}", rand::random::<u32>()));
        let _ = fs::create_dir_all(&temp_dir);
        let file1 = temp_dir.join("f1.bin");
        let file2 = temp_dir.join("f2.bin");
        fs::write(&file1, vec![0u8; 1024]).unwrap();
        fs::write(&file2, vec![0u8; 1024]).unwrap();

        let control = SessionControl::new();
        control.cancel(); // Pre-cancel
        let mut sink = DummySink;

        let err = prepare_payload(&[file1, file2], &control, &mut sink).expect_err("should cancel");
        match err {
            AppError::Cancelled => {}
            other => panic!("Expected AppError::Cancelled, got {other}"),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
