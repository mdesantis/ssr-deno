//! Minimal system type that satisfies the [`deno_runtime::deno_node::ExtNodeSys`]
//! and [`deno_permissions::which::WhichSys`] trait bounds.
//!
//! `ExtNodeSys` requires:
//! - `node_resolver::NodeResolverSys` (which is `FsCanonicalize + FsMetadata + FsRead + FsReadDir + FsOpen`)
//! - `sys_traits::EnvCurrentDir`
//! - `Clone`
//!
//! `WhichSys` requires:
//! - `sys_traits::EnvHomeDir`
//! - `sys_traits::EnvCurrentDir`
//! - `sys_traits::EnvVar` (via `BaseEnvVar`)
//! - `sys_traits::FsReadDir`
//! - `sys_traits::FsMetadata`
//! - `Clone`
//! - `'static`
//!
//! All methods delegate to the real filesystem / environment.

use std::path::Path;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Sys — the main system type
// ---------------------------------------------------------------------------

/// Minimal system type that delegates all filesystem and environment operations
/// to the real OS.
#[derive(Debug, Clone)]
pub struct Sys;

// ---------------------------------------------------------------------------
// FsCanonicalize (via BaseFsCanonicalize)
// ---------------------------------------------------------------------------

impl sys_traits::BaseFsCanonicalize for Sys {
    fn base_fs_canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        path.canonicalize()
    }
}

// ---------------------------------------------------------------------------
// FsMetadata (via BaseFsMetadata)
// ---------------------------------------------------------------------------

impl sys_traits::BaseFsMetadata for Sys {
    type Metadata = RealMetadata;

    fn base_fs_metadata(&self, path: &Path) -> std::io::Result<Self::Metadata> {
        std::fs::metadata(path).map(RealMetadata)
    }

    fn base_fs_symlink_metadata(&self, path: &Path) -> std::io::Result<Self::Metadata> {
        std::fs::symlink_metadata(path).map(RealMetadata)
    }
}

/// Wrapper around [`std::fs::Metadata`] that implements [`sys_traits::FsMetadataValue`].
#[derive(Debug)]
pub struct RealMetadata(pub std::fs::Metadata);

impl sys_traits::FsMetadataValue for RealMetadata {
    fn file_type(&self) -> sys_traits::FileType {
        self.0.file_type().into()
    }

    fn len(&self) -> u64 {
        self.0.len()
    }

    fn accessed(&self) -> std::io::Result<std::time::SystemTime> {
        self.0.accessed()
    }

    fn created(&self) -> std::io::Result<std::time::SystemTime> {
        self.0.created()
    }

    fn changed(&self) -> std::io::Result<std::time::SystemTime> {
        self.0.modified()
    }

    fn modified(&self) -> std::io::Result<std::time::SystemTime> {
        self.0.modified()
    }

    fn dev(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.dev())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn ino(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.ino())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn mode(&self) -> std::io::Result<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.mode())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn nlink(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.nlink())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn uid(&self) -> std::io::Result<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.uid())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn gid(&self) -> std::io::Result<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.gid())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn rdev(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.rdev())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn blksize(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.blksize())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn blocks(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(self.0.blocks())
        }
        #[cfg(not(unix))]
        {
            Ok(0)
        }
    }

    fn is_block_device(&self) -> std::io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            Ok(self.0.file_type().is_block_device())
        }
        #[cfg(not(unix))]
        {
            Ok(false)
        }
    }

    fn is_char_device(&self) -> std::io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            Ok(self.0.file_type().is_char_device())
        }
        #[cfg(not(unix))]
        {
            Ok(false)
        }
    }

    fn is_fifo(&self) -> std::io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            Ok(self.0.file_type().is_fifo())
        }
        #[cfg(not(unix))]
        {
            Ok(false)
        }
    }

    fn is_socket(&self) -> std::io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            Ok(self.0.file_type().is_socket())
        }
        #[cfg(not(unix))]
        {
            Ok(false)
        }
    }

    fn file_attributes(&self) -> std::io::Result<u32> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Ok(self.0.file_attributes())
        }
        #[cfg(not(windows))]
        {
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// FsRead (via BaseFsRead)
// ---------------------------------------------------------------------------

impl sys_traits::BaseFsRead for Sys {
    fn base_fs_read(&self, path: &Path) -> std::io::Result<std::borrow::Cow<'static, [u8]>> {
        std::fs::read(path).map(std::borrow::Cow::Owned)
    }
}

// ---------------------------------------------------------------------------
// FsReadDir (via BaseFsReadDir)
// ---------------------------------------------------------------------------

impl sys_traits::BaseFsReadDir for Sys {
    type ReadDirEntry = RealDirEntry;

    fn base_fs_read_dir(
        &self,
        path: &Path,
    ) -> std::io::Result<Box<dyn Iterator<Item = std::io::Result<Self::ReadDirEntry>>>> {
        let iter = std::fs::read_dir(path)?.map(|entry| entry.map(RealDirEntry));
        Ok(Box::new(iter))
    }
}

/// Wrapper around [`std::fs::DirEntry`] that implements [`sys_traits::FsDirEntry`].
#[derive(Debug)]
pub struct RealDirEntry(pub std::fs::DirEntry);

impl sys_traits::FsDirEntry for RealDirEntry {
    type Metadata = RealMetadata;

    fn file_name(&self) -> std::borrow::Cow<'_, std::ffi::OsStr> {
        self.0.file_name().into()
    }

    fn file_type(&self) -> std::io::Result<sys_traits::FileType> {
        self.0.file_type().map(Into::into)
    }

    fn metadata(&self) -> std::io::Result<Self::Metadata> {
        self.0.metadata().map(RealMetadata)
    }

    fn path(&self) -> std::borrow::Cow<'_, Path> {
        self.0.path().into()
    }
}

// ---------------------------------------------------------------------------
// FsOpen (via BaseFsOpen)
// ---------------------------------------------------------------------------

impl sys_traits::BaseFsOpen for Sys {
    type File = RealFile;

    fn base_fs_open(
        &self,
        path: &Path,
        options: &sys_traits::OpenOptions,
    ) -> std::io::Result<Self::File> {
        let mut std_opts = std::fs::OpenOptions::new();
        std_opts
            .read(options.read)
            .write(options.write)
            .create(options.create)
            .truncate(options.truncate)
            .append(options.append)
            .create_new(options.create_new);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Some(mode) = options.mode {
                std_opts.mode(mode);
            }
            if let Some(flags) = options.custom_flags {
                std_opts.custom_flags(flags);
            }
        }
        std_opts.open(path).map(RealFile)
    }
}

/// Wrapper around [`std::fs::File`] that implements [`sys_traits::FsFile`].
#[derive(Debug)]
pub struct RealFile(pub std::fs::File);

impl std::io::Read for RealFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for RealFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl std::io::Seek for RealFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl sys_traits::FsFileIsTerminal for RealFile {
    fn fs_file_is_terminal(&self) -> bool {
        use std::io::IsTerminal;
        self.0.is_terminal()
    }
}

impl sys_traits::FsFileLock for RealFile {
    fn fs_file_lock(&mut self, _mode: sys_traits::FsFileLockMode) -> std::io::Result<()> {
        unsupported_file_lock()
    }

    fn fs_file_try_lock(&mut self, _mode: sys_traits::FsFileLockMode) -> std::io::Result<()> {
        unsupported_file_lock()
    }

    fn fs_file_unlock(&mut self) -> std::io::Result<()> {
        unsupported_file_lock()
    }
}

/// Returns an "unsupported" I/O error for file locking operations.
fn unsupported_file_lock() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "file locking not supported",
    ))
}

impl sys_traits::FsFileMetadata for RealFile {
    fn fs_file_metadata(&self) -> std::io::Result<sys_traits::boxed::BoxedFsMetadataValue> {
        self.0
            .metadata()
            .map(|m| sys_traits::boxed::BoxedFsMetadataValue::new(RealMetadata(m)))
    }
}

impl sys_traits::FsFileSetLen for RealFile {
    fn fs_file_set_len(&mut self, size: u64) -> std::io::Result<()> {
        self.0.set_len(size)
    }
}

impl sys_traits::FsFileSetPermissions for RealFile {
    fn fs_file_set_permissions(&mut self, mode: u32) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            self.0
                .set_permissions(std::fs::Permissions::from_mode(mode))
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "setting permissions not supported",
            ))
        }
    }
}

impl sys_traits::FsFileSetTimes for RealFile {
    fn fs_file_set_times(&mut self, _times: sys_traits::FsFileTimes) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "setting file times not supported",
        ))
    }
}

impl sys_traits::FsFileSyncAll for RealFile {
    fn fs_file_sync_all(&mut self) -> std::io::Result<()> {
        self.0.sync_all()
    }
}

impl sys_traits::FsFileSyncData for RealFile {
    fn fs_file_sync_data(&mut self) -> std::io::Result<()> {
        self.0.sync_data()
    }
}

impl sys_traits::FsFileAsRaw for RealFile {
    #[cfg(unix)]
    fn fs_file_as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        use std::os::unix::io::AsRawFd;
        Some(self.0.as_raw_fd())
    }

    #[cfg(windows)]
    fn fs_file_as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        use std::os::windows::io::AsRawHandle;
        Some(self.0.as_raw_handle())
    }
}

// Blanket impl: FsFile requires all the above traits, which we've implemented.
impl sys_traits::FsFile for RealFile {}

// ---------------------------------------------------------------------------
// EnvCurrentDir
// ---------------------------------------------------------------------------

impl sys_traits::EnvCurrentDir for Sys {
    fn env_current_dir(&self) -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }
}

// ---------------------------------------------------------------------------
// EnvHomeDir (required by WhichSys)
// ---------------------------------------------------------------------------

impl sys_traits::EnvHomeDir for Sys {
    fn env_home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

// ---------------------------------------------------------------------------
// BaseEnvVar (required by WhichSys via EnvVar)
// ---------------------------------------------------------------------------

impl sys_traits::BaseEnvVar for Sys {
    fn base_env_var_os(&self, key: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
        std::env::var_os(key)
    }
}

// NodeResolverSys and ExtNodeSys have blanket impls in their respective
// crates — Sys already satisfies all supertraits via the Base* impls above.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}
