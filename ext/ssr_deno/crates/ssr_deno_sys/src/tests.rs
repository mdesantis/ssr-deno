use super::*;
use std::io::{Read, Seek, SeekFrom, Write};
use sys_traits::OpenOptions;
use sys_traits::{
    BaseEnvVar, BaseFsCanonicalize, BaseFsMetadata, BaseFsOpen, BaseFsRead, BaseFsReadDir,
    EnvCurrentDir, EnvHomeDir, FsDirEntry, FsFileIsTerminal, FsFileLock, FsFileLockMode,
    FsFileMetadata, FsFileSetLen, FsFileSetPermissions, FsFileSetTimes, FsFileSyncAll,
    FsFileSyncData, FsFileTimes, FsMetadataValue,
};

// Helper: return a unique path in the system temp dir for the given test name
fn tmp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ssr_deno_sys_test__{name}"))
}

// Helper: create a temp file with given content and return the path
fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
    let path = tmp(name);
    std::fs::write(&path, content).expect("write_tmp failed");
    path
}

// Helper: clean up a temp path (ignore errors)
fn remove_tmp(path: &std::path::PathBuf) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(path);
}

// -----------------------------------------------------------------------
// Sys: Debug, Clone
// -----------------------------------------------------------------------

#[test]
fn sys_debug_and_clone() {
    let sys = Sys;
    let cloned = sys.clone();
    let debug_str = format!("{sys:?}");
    assert!(debug_str.contains("Sys"));
    let _ = cloned; // just show it compiles and runs
}

// -----------------------------------------------------------------------
// BaseFsCanonicalize
// -----------------------------------------------------------------------

#[test]
fn canonicalize_existing_file() {
    let path = write_tmp("canonicalize_existing", b"hello");
    let sys = Sys;
    let canonical = sys.base_fs_canonicalize(&path).unwrap();
    assert!(canonical.is_absolute());
    remove_tmp(&path);
}

#[test]
fn canonicalize_nonexistent_file() {
    let path = tmp("canonicalize_nonexistent_xyz_never_created");
    let sys = Sys;
    let result = sys.base_fs_canonicalize(&path);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// BaseFsMetadata + RealMetadata
// -----------------------------------------------------------------------

#[test]
fn metadata_file_type_is_file() {
    let path = write_tmp("metadata_file_type", b"data");
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();
    assert_eq!(meta.file_type(), sys_traits::FileType::File);
    remove_tmp(&path);
}

#[test]
fn metadata_dir_type_is_dir() {
    let dir = tmp("metadata_dir_type");
    std::fs::create_dir_all(&dir).unwrap();
    let sys = Sys;
    let meta = sys.base_fs_metadata(&dir).unwrap();
    assert_eq!(meta.file_type(), sys_traits::FileType::Dir);
    remove_tmp(&dir);
}

#[test]
fn metadata_len() {
    let content = b"hello world";
    let path = write_tmp("metadata_len", content);
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();
    assert_eq!(meta.len(), content.len() as u64);
    remove_tmp(&path);
}

#[test]
fn metadata_times() {
    let path = write_tmp("metadata_times", b"x");
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();
    // modified and changed should succeed on Linux
    assert!(meta.modified().is_ok());
    assert!(meta.changed().is_ok());
    // accessed may or may not succeed depending on mount options
    let _ = meta.accessed();
    // created may return Err on Linux (not all filesystems support it)
    let _ = meta.created();
    remove_tmp(&path);
}

#[test]
fn metadata_unix_fields() {
    let path = write_tmp("metadata_unix_fields", b"unix");
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();

    #[cfg(unix)]
    {
        // dev and ino should be nonzero for a real file
        assert!(meta.dev().unwrap() > 0);
        assert!(meta.ino().unwrap() > 0);
        // mode should have some bits set
        assert!(meta.mode().unwrap() > 0);
        // nlink should be at least 1
        assert!(meta.nlink().unwrap() >= 1);
        // uid and gid should be readable (could be 0 if root)
        assert!(meta.uid().is_ok());
        assert!(meta.gid().is_ok());
        // rdev is 0 for regular files
        assert_eq!(meta.rdev().unwrap(), 0);
        // blksize > 0 on Linux
        assert!(meta.blksize().unwrap() > 0);
        // blocks >= 0
        assert!(meta.blocks().is_ok());
        // a regular file is not a block/char device/fifo/socket
        assert!(!meta.is_block_device().unwrap());
        assert!(!meta.is_char_device().unwrap());
        assert!(!meta.is_fifo().unwrap());
        assert!(!meta.is_socket().unwrap());
    }

    #[cfg(not(unix))]
    {
        // On non-unix these all return Ok(0) or Ok(false)
        assert_eq!(meta.dev().unwrap(), 0);
        assert_eq!(meta.ino().unwrap(), 0);
        assert_eq!(meta.mode().unwrap(), 0);
        assert_eq!(meta.nlink().unwrap(), 0);
        assert_eq!(meta.uid().unwrap(), 0);
        assert_eq!(meta.gid().unwrap(), 0);
        assert_eq!(meta.rdev().unwrap(), 0);
        assert_eq!(meta.blksize().unwrap(), 0);
        assert_eq!(meta.blocks().unwrap(), 0);
        assert!(!meta.is_block_device().unwrap());
        assert!(!meta.is_char_device().unwrap());
        assert!(!meta.is_fifo().unwrap());
        assert!(!meta.is_socket().unwrap());
    }

    remove_tmp(&path);
}

#[test]
fn metadata_file_attributes() {
    let path = write_tmp("metadata_file_attrs", b"attrs");
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();
    #[cfg(windows)]
    assert!(meta.file_attributes().is_ok());
    #[cfg(not(windows))]
    assert_eq!(meta.file_attributes().unwrap(), 0);
    remove_tmp(&path);
}

#[test]
fn metadata_debug() {
    let path = write_tmp("metadata_debug", b"debug");
    let sys = Sys;
    let meta = sys.base_fs_metadata(&path).unwrap();
    let s = format!("{meta:?}");
    assert!(!s.is_empty());
    remove_tmp(&path);
}

#[test]
fn symlink_metadata() {
    let path = write_tmp("symlink_meta_target", b"target");
    let link = tmp("symlink_meta_link");
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&path, &link).unwrap();
    let sys = Sys;
    let meta = sys.base_fs_symlink_metadata(&link).unwrap();
    assert_eq!(meta.file_type(), sys_traits::FileType::Symlink);
    remove_tmp(&link);
    remove_tmp(&path);
}

#[test]
fn metadata_nonexistent() {
    let path = tmp("metadata_nonexistent_xyz_never_created");
    let sys = Sys;
    assert!(sys.base_fs_metadata(&path).is_err());
}

// -----------------------------------------------------------------------
// BaseFsRead
// -----------------------------------------------------------------------

#[test]
fn fs_read_existing() {
    let content = b"some content";
    let path = write_tmp("fs_read_existing", content);
    let sys = Sys;
    let got = sys.base_fs_read(&path).unwrap();
    assert_eq!(got.as_ref(), content);
    remove_tmp(&path);
}

#[test]
fn fs_read_nonexistent() {
    let path = tmp("fs_read_nonexistent_xyz_never_created");
    let sys = Sys;
    assert!(sys.base_fs_read(&path).is_err());
}

// -----------------------------------------------------------------------
// BaseFsReadDir + RealDirEntry
// -----------------------------------------------------------------------

#[test]
fn read_dir_lists_files() {
    let dir = tmp("read_dir_lists_files_dir");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("child.txt");
    std::fs::write(&file, b"hi").unwrap();

    let sys = Sys;
    let entries: Vec<_> = sys
        .base_fs_read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();

    assert!(!entries.is_empty());
    let found = entries
        .iter()
        .any(|e| e.file_name().to_string_lossy().contains("child.txt"));
    assert!(found);

    // test all FsDirEntry methods on the first matching entry
    let entry = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().contains("child.txt"))
        .unwrap();

    assert_eq!(entry.file_type().unwrap(), sys_traits::FileType::File);
    let meta = entry.metadata().unwrap();
    assert_eq!(meta.len(), 2);
    let entry_path = entry.path();
    assert!(entry_path.ends_with("child.txt"));

    // Debug
    let dbg = format!("{entry:?}");
    assert!(!dbg.is_empty());

    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn read_dir_nonexistent() {
    let path = tmp("read_dir_nonexistent_xyz_never_created");
    let sys = Sys;
    assert!(sys.base_fs_read_dir(&path).is_err());
}

// -----------------------------------------------------------------------
// BaseFsOpen + RealFile
// -----------------------------------------------------------------------

fn open_for_write(path: &std::path::PathBuf) -> RealFile {
    let sys = Sys;
    let opts = OpenOptions::new_write();
    sys.base_fs_open(path, &opts).unwrap()
}

fn open_for_read(path: &std::path::PathBuf) -> RealFile {
    let sys = Sys;
    let opts = OpenOptions::new_read();
    sys.base_fs_open(path, &opts).unwrap()
}

fn open_for_read_write(path: &std::path::PathBuf) -> RealFile {
    let sys = Sys;
    // read + write + create (no truncate so existing content is preserved)
    let mut opts = OpenOptions::new();
    opts.read = true;
    opts.write = true;
    opts.create = true;
    sys.base_fs_open(path, &opts).unwrap()
}

#[test]
fn file_write_and_read() {
    let path = tmp("file_write_and_read");
    {
        let mut f = open_for_write(&path);
        f.write_all(b"hello, world!").unwrap();
        f.flush().unwrap();
    }
    {
        let mut f = open_for_read(&path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"hello, world!");
    }
    remove_tmp(&path);
}

#[test]
#[cfg(unix)]
fn file_open_with_unix_mode() {
    let path = tmp("open_with_mode");
    let mut opts = OpenOptions::new();
    opts.read = true;
    opts.write = true;
    opts.create = true;
    opts.mode = Some(0o644);
    let sys = Sys;
    let f = sys.base_fs_open(&path, &opts).unwrap();
    drop(f);
    remove_tmp(&path);
}

#[test]
#[cfg(unix)]
fn file_open_with_unix_custom_flags() {
    let path = tmp("open_with_flags");
    let mut opts = OpenOptions::new();
    opts.read = true;
    opts.write = true;
    opts.create = true;
    opts.custom_flags = Some(0);
    let sys = Sys;
    let f = sys.base_fs_open(&path, &opts).unwrap();
    drop(f);
    remove_tmp(&path);
}

#[test]
fn file_seek() {
    let path = tmp("file_seek");
    {
        let mut f = open_for_write(&path);
        f.write_all(b"abcdefgh").unwrap();
    }
    {
        let mut f = open_for_read(&path);
        // seek to offset 3
        let pos = f.seek(SeekFrom::Start(3)).unwrap();
        assert_eq!(pos, 3);
        let mut buf = [0u8; 3];
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"def");
    }
    remove_tmp(&path);
}

#[test]
fn file_is_terminal_is_false_for_regular_file() {
    let path = write_tmp("file_is_terminal", b"x");
    let f = open_for_read(&path);
    // Regular files on disk are not terminals
    assert!(!f.fs_file_is_terminal());
    remove_tmp(&path);
}

#[test]
fn file_lock_returns_unsupported() {
    let path = write_tmp("file_lock_unsupported", b"x");
    let mut f = open_for_read(&path);
    let err = f.fs_file_lock(FsFileLockMode::Shared).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    remove_tmp(&path);
}

#[test]
fn file_try_lock_returns_unsupported() {
    let path = write_tmp("file_try_lock_unsupported", b"x");
    let mut f = open_for_read(&path);
    let err = f.fs_file_try_lock(FsFileLockMode::Exclusive).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    remove_tmp(&path);
}

#[test]
fn file_unlock_returns_unsupported() {
    let path = write_tmp("file_unlock_unsupported", b"x");
    let mut f = open_for_read(&path);
    let err = f.fs_file_unlock().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    remove_tmp(&path);
}

#[test]
fn file_set_times_returns_unsupported() {
    let path = write_tmp("file_set_times_unsupported", b"x");
    let mut f = open_for_read(&path);
    let times = FsFileTimes::new();
    let err = f.fs_file_set_times(times).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    remove_tmp(&path);
}

#[test]
fn file_metadata() {
    let content = b"metadata content";
    let path = write_tmp("file_metadata", content);
    let f = open_for_read(&path);
    let meta = f.fs_file_metadata().unwrap();
    assert_eq!(meta.len(), content.len() as u64);
    remove_tmp(&path);
}

#[test]
fn file_set_len() {
    let path = tmp("file_set_len");
    {
        let mut f = open_for_write(&path);
        f.write_all(b"12345678").unwrap();
    }
    {
        let mut f = open_for_read_write(&path);
        f.fs_file_set_len(4).unwrap();
    }
    let content = std::fs::read(&path).unwrap();
    assert_eq!(content, b"1234");
    remove_tmp(&path);
}

#[test]
fn file_set_permissions() {
    let path = write_tmp("file_set_permissions", b"perms");
    let mut f = open_for_read_write(&path);
    #[cfg(unix)]
    {
        // set read-only for owner
        f.fs_file_set_permissions(0o444).unwrap();
        // restore write permission before removing
        f.fs_file_set_permissions(0o644).unwrap();
    }
    #[cfg(not(unix))]
    {
        let err = f.fs_file_set_permissions(0o644).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }
    drop(f);
    remove_tmp(&path);
}

#[test]
fn file_sync_all() {
    let path = tmp("file_sync_all");
    let mut f = open_for_write(&path);
    f.write_all(b"sync all").unwrap();
    f.fs_file_sync_all().unwrap();
    drop(f);
    remove_tmp(&path);
}

#[test]
fn file_sync_data() {
    let path = tmp("file_sync_data");
    let mut f = open_for_write(&path);
    f.write_all(b"sync data").unwrap();
    f.fs_file_sync_data().unwrap();
    drop(f);
    remove_tmp(&path);
}

#[test]
#[cfg(unix)]
fn file_as_raw_fd() {
    use sys_traits::FsFileAsRaw;
    let path = write_tmp("file_as_raw_fd", b"fd test");
    let f = open_for_read(&path);
    let fd = f.fs_file_as_raw_fd();
    assert!(fd.is_some());
    assert!(fd.unwrap() >= 0);
    remove_tmp(&path);
}

#[test]
fn file_debug() {
    let path = write_tmp("file_debug", b"debug");
    let f = open_for_read(&path);
    let s = format!("{f:?}");
    assert!(!s.is_empty());
    remove_tmp(&path);
}

#[test]
fn open_nonexistent_read_only_fails() {
    let path = tmp("open_nonexistent_read_only_fails_xyz");
    let sys = Sys;
    let opts = OpenOptions::new_read();
    assert!(sys.base_fs_open(&path, &opts).is_err());
}

// -----------------------------------------------------------------------
// unsupported_file_lock (private fn — tested indirectly via FsFileLock)
// -----------------------------------------------------------------------

#[test]
fn unsupported_file_lock_error_kind() {
    // Access via the public FsFileLock impl on RealFile, which calls
    // the private `unsupported_file_lock()` for all three methods.
    let path = write_tmp("unsupported_file_lock_direct", b"x");
    let mut f = open_for_read(&path);

    let e1 = f.fs_file_lock(FsFileLockMode::Shared).unwrap_err();
    assert_eq!(e1.kind(), std::io::ErrorKind::Unsupported);

    let e2 = f.fs_file_try_lock(FsFileLockMode::Exclusive).unwrap_err();
    assert_eq!(e2.kind(), std::io::ErrorKind::Unsupported);

    let e3 = f.fs_file_unlock().unwrap_err();
    assert_eq!(e3.kind(), std::io::ErrorKind::Unsupported);

    remove_tmp(&path);
}

// -----------------------------------------------------------------------
// EnvCurrentDir
// -----------------------------------------------------------------------

#[test]
fn env_current_dir_is_absolute() {
    let sys = Sys;
    let cwd = sys.env_current_dir().unwrap();
    assert!(cwd.is_absolute());
}

// -----------------------------------------------------------------------
// EnvHomeDir
// -----------------------------------------------------------------------

#[test]
fn env_home_dir_on_unix() {
    #[cfg(unix)]
    {
        // Exercise Some branch (HOME is normally set)
        let sys = Sys;
        let home = sys.env_home_dir();
        if let Some(h) = home {
            assert!(h.is_absolute() || !h.as_os_str().is_empty());
        }
        // Exercise None branch by temporarily removing HOME
        let old_home = std::env::var_os("HOME");
        std::env::remove_var("HOME");
        let home2 = sys.env_home_dir();
        assert!(home2.is_none());
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        }
    }
    #[cfg(not(unix))]
    {
        // On Windows, HOME is typically not set
        let sys = Sys;
        assert!(sys.env_home_dir().is_none());
    }
}

// -----------------------------------------------------------------------
// BaseEnvVar
// -----------------------------------------------------------------------

#[test]
fn env_var_path_exists() {
    let sys = Sys;
    let path = sys.base_env_var_os(std::ffi::OsStr::new("PATH"));
    assert!(path.is_some(), "PATH env var should always be set");
}

#[test]
fn env_var_nonexistent_returns_none() {
    let sys = Sys;
    let val = sys.base_env_var_os(std::ffi::OsStr::new("SSR_DENO_NONEXISTENT_VAR_XYZ"));
    assert!(val.is_none());
}
