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
        use std::os::unix::io::AsRawFd;
        // SAFETY: isatty is safe to call with a valid fd
        unsafe { libc::isatty(self.0.as_raw_fd()) != 0 }
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
