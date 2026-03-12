use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A sysfs/procfs file handle that stays open across poll cycles.
///
/// Instead of open→read→close on every poll, this seeks to 0 and re-reads,
/// saving two syscalls per read. Safe for sysfs attributes where the kernel
/// regenerates content on each read.
pub struct CachedFile {
    file: File,
    buf: String,
}

impl CachedFile {
    /// Open a file for cached reading. Returns `None` if the file doesn't exist
    /// or can't be opened.
    pub fn open(path: impl AsRef<Path>) -> Option<Self> {
        let file = File::open(path).ok()?;
        Some(Self {
            file,
            buf: String::with_capacity(32),
        })
    }

    /// Seek to start and read file contents into internal buffer.
    fn refresh(&mut self) -> bool {
        self.buf.clear();
        if self.file.seek(SeekFrom::Start(0)).is_err() {
            return false;
        }
        self.file.read_to_string(&mut self.buf).is_ok()
    }

    /// Read and parse as u64 (supports 0x hex prefix).
    pub fn read_u64(&mut self) -> Option<u64> {
        if !self.refresh() {
            return None;
        }
        parse_int_flexible(self.buf.trim()).ok()
    }

    /// Read trimmed content as a new String.
    pub fn read_string(&mut self) -> Option<String> {
        if !self.refresh() {
            return None;
        }
        let trimmed = self.buf.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Read and return a reference to the trimmed buffer content.
    /// The reference is valid until the next read call.
    pub fn read_raw(&mut self) -> Option<&str> {
        if !self.refresh() {
            return None;
        }
        let s = self.buf.trim();
        if s.is_empty() { None } else { Some(s) }
    }
}

pub fn read_string_optional(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty()
            || trimmed == "N/A"
            || trimmed == "To Be Filled By O.E.M."
            || trimmed == "Default string"
            || trimmed == "Not Specified"
        {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub fn read_u64_optional(path: &Path) -> Option<u64> {
    read_string_optional(path).and_then(|s| parse_int_flexible(&s).ok())
}

pub fn read_u32_optional(path: &Path) -> Option<u32> {
    read_u64_optional(path).map(|v| v as u32)
}

pub fn read_link_basename(path: &Path) -> Option<String> {
    fs::read_link(path)
        .ok()
        .and_then(|target| target.file_name().map(|n| n.to_string_lossy().to_string()))
}

pub fn glob_paths(pattern: &str) -> Vec<PathBuf> {
    glob::glob(pattern)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .collect()
}

fn parse_int_flexible(s: &str) -> Result<u64, std::num::ParseIntError> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp file with the given content and return its path.
    /// Uses a PID-scoped directory to reduce name collisions between tests.
    fn write_temp(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("siomon_test_{}", std::process::id()));
        let _ = fs::create_dir(&dir);
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn cached_file_open_nonexistent() {
        assert!(CachedFile::open("/nonexistent/siomon_test_12345").is_none());
    }

    #[test]
    fn cached_file_read_u64() {
        let path = write_temp("read_u64", "42\n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_u64(), Some(42));
    }

    #[test]
    fn cached_file_read_u64_hex() {
        let path = write_temp("read_u64_hex", "0x1A2B\n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_u64(), Some(0x1A2B));
    }

    #[test]
    fn cached_file_read_string_trims() {
        let path = write_temp("read_string", "  hello world  \n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_string().as_deref(), Some("hello world"));
    }

    #[test]
    fn cached_file_read_string_empty_returns_none() {
        let path = write_temp("read_string_empty", "  \n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_string(), None);
    }

    #[test]
    fn cached_file_read_raw() {
        let path = write_temp("read_raw", "  raw_content  \n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_raw(), Some("raw_content"));
    }

    #[test]
    fn cached_file_rereads_after_external_write() {
        let path = write_temp("reread", "100\n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_u64(), Some(100));

        // Overwrite file content (simulates kernel updating a sysfs attr)
        fs::write(&path, "200\n").unwrap();
        assert_eq!(cf.read_u64(), Some(200));
    }

    #[test]
    fn cached_file_multiple_reads_same_handle() {
        let path = write_temp("multi_read", "999\n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_u64(), Some(999));
        assert_eq!(cf.read_u64(), Some(999));
        assert_eq!(cf.read_string().as_deref(), Some("999"));
    }

    #[test]
    fn cached_file_non_numeric_returns_none() {
        let path = write_temp("non_numeric", "not_a_number\n");
        let mut cf = CachedFile::open(&path).unwrap();
        assert_eq!(cf.read_u64(), None);
    }
}
