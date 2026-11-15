//! The few OS-specific bits: free disk space, tmpfs detection, PATH lookup.

use std::path::{Path, PathBuf};

/// Free space (MiB) on the filesystem holding `path`, if it can be determined.
#[cfg(unix)]
pub fn free_space_mb(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` is a
    // properly sized, writable statvfs struct that the call fully initializes.
    let stat = unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
            return None;
        }
        stat
    };
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64) / 1_048_576)
}

#[cfg(not(unix))]
pub fn free_space_mb(_path: &Path) -> Option<u64> {
    None
}

/// Resident set size (RSS in MiB) of the current process, if available.
pub fn process_memory_mb() -> Option<u64> {
    #[cfg(unix)]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(pages) = s.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()) {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                if page_size > 0 {
                    return Some((pages.saturating_mul(page_size as u64)) / 1_048_576);
                }
            }
        }
    }
    None
}

/// True if `path` lives on a RAM-backed filesystem (Linux only). Downloading
/// there silently eats memory instead of disk.
pub fn is_tmpfs(path: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    fs_type_of(&mounts, &path).is_some_and(|t| t == "tmpfs" || t == "ramfs")
}

/// Filesystem type of the longest mount point that is a prefix of `path`.
fn fs_type_of(mountinfo: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        // "36 35 98:0 /root /mount/point opts... - fstype source superopts"
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let Some(mount_point) = left.split(' ').nth(4) else { continue };
        let Some(fs_type) = right.split(' ').next() else { continue };
        if path.starts_with(mount_point) && best.as_ref().is_none_or(|(len, _)| mount_point.len() >= *len) {
            best = Some((mount_point.len(), fs_type.to_string()));
        }
    }
    best.map(|(_, t)| t)
}

/// Locate an executable the way a shell would.
pub fn which(bin: &str) -> Option<PathBuf> {
    let candidate = Path::new(bin);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for ext in ["", ".exe", ".cmd", ".bat"] {
            let full = dir.join(format!("{bin}{ext}"));
            if full.is_file() {
                return Some(full);
            }
            if !cfg!(windows) {
                break;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_lookup() {
        let info = "\
22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
30 22 0:26 / /tmp rw,nosuid,nodev shared:5 - tmpfs tmpfs rw
41 22 8:2 / /var/lib/docker/volumes rw - xfs /dev/sdb1 rw
";
        assert_eq!(fs_type_of(info, Path::new("/tmp/tg-media-bot")).as_deref(), Some("tmpfs"));
        assert_eq!(fs_type_of(info, Path::new("/data/tmp")).as_deref(), Some("ext4"));
        assert_eq!(fs_type_of(info, Path::new("/var/lib/docker/volumes/x")).as_deref(), Some("xfs"));
        // "/tmpfoo" must not match the "/tmp" mount.
        assert_eq!(fs_type_of(info, Path::new("/tmpfoo/x")).as_deref(), Some("ext4"));
    }
}
