//! File snapshot helpers — V1.1 §6.2.
//!
//! Captures the four-field snapshot used in `EffectManifest`:
//!   - canonical_path: via fs_paths::canonicalize()
//!   - file_id: OS-level file identity (Win: volume+index, via GetFileInformationByHandle)
//!   - size: file length in bytes
//!   - last_write_time: RFC3339 string
//!   - sha256: hex digest prefixed with "sha256:"
//!
//! These fields feed into `preconditions_hash` for TOCTOU detection.

use crate::error::{KernelError, Result};
use crate::tools::fs_paths::canonicalize;
use chrono::{DateTime, Utc};
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub canonical_path: String,
    pub file_id: String,
    pub size: u64,
    pub last_write_time: String, // RFC3339
    pub sha256: String,          // "sha256:<hex>"
}

/// Snapshot a single file. Returns KernelError::Io if the path is missing or not a regular file.
pub fn snapshot_file(path: &Path) -> Result<FileSnapshot> {
    let meta = fs::metadata(path).map_err(KernelError::Io)?;
    if !meta.is_file() {
        return Err(KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("not a regular file: {}", path.display()),
        )));
    }

    let canonical_path = canonicalize(&path.to_string_lossy());
    let size = meta.len();
    let last_write_time = format_rfc3339(meta.modified().map_err(KernelError::Io)?);
    let file_id = file_identity(path, &meta)?;
    let sha256 = sha256_of_file(path)?;

    Ok(FileSnapshot {
        canonical_path,
        file_id,
        size,
        last_write_time,
        sha256,
    })
}

/// Snapshot a list of files. Returns error on the first failure.
pub fn snapshot_files(paths: &[&Path]) -> Result<Vec<FileSnapshot>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(snapshot_file(p)?);
    }
    Ok(out)
}

fn sha256_of_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(KernelError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf).map_err(KernelError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn format_rfc3339(time: SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    dt.to_rfc3339()
}

#[cfg(windows)]
fn file_identity(path: &Path, _meta: &fs::Metadata) -> Result<String> {
    // The std::os::windows::fs::MetadataExt methods `volume_serial_number`,
    // `file_index_high`, and `file_index_low` are gated behind the unstable
    // `windows_by_handle` feature. We call GetFileInformationByHandle via FFI
    // to obtain the same BY_HANDLE_FILE_INFORMATION fields without a nightly
    // compiler or an extra windows-sys dependency.
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    #[allow(non_snake_case)]
    struct ByHandleFileInformation {
        dwFileAttributes: u32,
        ftCreationTime: FileTime,
        ftLastAccessTime: FileTime,
        ftLastWriteTime: FileTime,
        dwVolumeSerialNumber: u32,
        nFileSizeHigh: u32,
        nFileSizeLow: u32,
        nNumberOfLinks: u32,
        nFileIndexHigh: u32,
        nFileIndexLow: u32,
    }

    unsafe extern "system" {
        fn GetFileInformationByHandle(
            h_file: *mut core::ffi::c_void,
            lp_file_information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let f = fs::File::open(path).map_err(KernelError::Io)?;
    let mut info: ByHandleFileInformation = Default::default();
    let rc = unsafe {
        GetFileInformationByHandle(f.as_raw_handle() as *mut _, &mut info)
    };
    if rc == 0 {
        return Err(KernelError::Io(std::io::Error::last_os_error()));
    }
    // Combine volume serial, file index high/low. Lower 16 bits of nFileIndexHigh
    // are reused for reparse tags on some NTFS builds; we include the full 64-bit
    // index which is stable for non-reparse files.
    Ok(format!(
        "win:{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}

// Suppress unused-import warning for SqlValue on non-windows; we keep it for future use.
#[allow(dead_code)]
fn _touch_sqlvalue() -> SqlValue {
    SqlValue::Null
}
