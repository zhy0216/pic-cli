//! Directory-relative, no-follow I/O. Only the manifest publishes history.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{
    AtFlags, CWD, FlockOperation, Mode, OFlags, RenameFlags, flock, mkdirat, openat, renameat,
    renameat_with, unlinkat,
};
use sha2::{Digest, Sha256};

use crate::{ErrorCode, PicError, Result};

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn check_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(PicError::new(
            ErrorCode::UnsafePath,
            "asset/ops references must be lowercase SHA-256 digests, never paths",
        ));
    }
    Ok(())
}

fn io(action: &str, path: &Path, error: rustix::io::Errno) -> PicError {
    if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
        PicError::new(
            ErrorCode::UnsafePath,
            format!(
                "{action} '{}': symlink or invalid directory",
                path.display()
            ),
        )
    } else {
        PicError::io(action, path, error.into())
    }
}

pub(super) fn project_path(path: &Path) -> Result<PathBuf> {
    if path.to_str().is_none() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "project path must be UTF-8",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| PicError::new(ErrorCode::UnsafePath, "project must name a directory"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent =
        fs::canonicalize(parent).map_err(|e| PicError::io("resolve project parent", parent, e))?;
    if parent.to_str().is_none() {
        return Err(PicError::new(
            ErrorCode::InvalidArgument,
            "project parent must be UTF-8",
        ));
    }
    Ok(parent.join(name))
}

fn directory(parent: &impl std::os::fd::AsFd, path: &Path) -> Result<File> {
    openat(
        parent,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| io("open project directory", path, e))
}

pub(super) struct Storage {
    pub path: PathBuf,
    root: File,
    assets: File,
    ops: File,
}

impl Storage {
    pub fn initialize(path: &Path) -> Result<Self> {
        let root = directory(&CWD, path)?;
        for name in ["assets", "ops"] {
            mkdirat(&root, name, Mode::from_raw_mode(0o700))
                .map_err(|e| io("create project directory", &path.join(name), e))?;
        }
        // Never replace or delete this inode: all writers lock the same file.
        write_atomic(&root, ".lock", b"", false, "lock_write")?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let path = project_path(path)?;
        let root = directory(&CWD, &path)?;
        let assets = directory(&root, Path::new("assets"))?;
        let ops = directory(&root, Path::new("ops"))?;
        Ok(Self {
            path,
            root,
            assets,
            ops,
        })
    }

    pub fn lock(&self) -> Result<File> {
        let file = open_file(&self.root, ".lock")?;
        flock(&file, FlockOperation::LockExclusive)
            .map_err(|e| io("lock project", &self.path, e))?;
        Ok(file)
    }

    pub fn manifest(&self, limit: u64) -> Result<Vec<u8>> {
        read(&self.root, "manifest.json", limit)
    }

    pub fn publish_manifest(&self, bytes: &[u8], initial: bool) -> Result<()> {
        write_atomic(
            &self.root,
            "manifest.json",
            bytes,
            !initial,
            "manifest_write",
        )
    }

    pub fn asset(&self, digest: &str, limit: u64) -> Result<Vec<u8>> {
        check_hash(digest)?;
        let bytes = read(&self.assets, digest, limit).map_err(|mut error| {
            if error.code == ErrorCode::FileNotFound {
                error.code = ErrorCode::AssetMissing;
                error.message = format!("required asset {digest} is missing");
            }
            error
        })?;
        verify(digest, &bytes)?;
        Ok(bytes)
    }

    pub fn put_asset(&self, bytes: &[u8]) -> Result<String> {
        put(&self.assets, bytes, false, "asset_write")
    }

    pub fn commit(&self, digest: &str, limit: u64) -> Result<Vec<u8>> {
        check_hash(digest)?;
        let bytes = read(&self.ops, &format!("{digest}.json"), limit)?;
        verify(digest, &bytes)?;
        Ok(bytes)
    }

    pub fn put_commit(&self, bytes: &[u8]) -> Result<String> {
        put(&self.ops, bytes, true, "ops_write")
    }
}

pub(super) fn publish_directory(staged: &Path, destination: &Path) -> Result<()> {
    fail("create_publish")?;
    // Atomic no-clobber also protects against another creator or an empty existing directory.
    renameat_with(CWD, staged, CWD, destination, RenameFlags::NOREPLACE)
        .map_err(|e| io("publish project", destination, e))
}

fn verify(digest: &str, bytes: &[u8]) -> Result<()> {
    if hash(bytes) != digest {
        return Err(PicError::new(
            ErrorCode::IntegrityMismatch,
            format!("SHA-256 mismatch for {digest}"),
        ));
    }
    Ok(())
}

fn put(dir: &File, bytes: &[u8], json: bool, stage: &'static str) -> Result<String> {
    let digest = hash(bytes);
    let name = if json {
        format!("{digest}.json")
    } else {
        digest.clone()
    };
    match write_atomic(dir, &name, bytes, false, stage) {
        Ok(()) => (),
        Err(error) if error.code == ErrorCode::OutputExists => {
            let existing = read(dir, &name, bytes.len() as u64)?;
            verify(&digest, &existing)?;
        }
        Err(error) => return Err(error),
    }
    Ok(digest)
}

fn open_file(dir: &File, name: &str) -> Result<File> {
    let file = openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| io("open project file", Path::new(name), e))?;
    if !file
        .metadata()
        .map_err(|e| PicError::io("inspect project file", Path::new(name), e))?
        .is_file()
    {
        return Err(PicError::new(
            ErrorCode::UnsafePath,
            format!("project file {name} must be a regular file"),
        ));
    }
    Ok(file)
}

fn read(dir: &File, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = open_file(dir, name)?;
    if file
        .metadata()
        .map_err(|e| PicError::io("inspect project file", Path::new(name), e))?
        .len()
        > limit
    {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            format!("project file {name} exceeds byte limit"),
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| PicError::io("read project file", Path::new(name), e))?;
    if bytes.len() as u64 > limit {
        return Err(PicError::new(
            ErrorCode::ResourceLimit,
            "project file grew beyond byte limit",
        ));
    }
    Ok(bytes)
}

struct Temporary<'a> {
    directory: &'a File,
    name: String,
}

impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
    }
}

fn write_atomic(
    dir: &File,
    name: &str,
    bytes: &[u8],
    replace: bool,
    stage: &'static str,
) -> Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let name = format!(
            ".pic-tmp-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        match openat(
            dir,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => {
                break (
                    Temporary {
                        directory: dir,
                        name,
                    },
                    File::from(fd),
                );
            }
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => return Err(io("create project temporary", Path::new(name.as_str()), e)),
        }
    };
    let middle = bytes.len() / 2;
    file.write_all(&bytes[..middle])
        .map_err(|e| PicError::io("write project temporary", Path::new(name), e))?;
    fail(stage)?;
    file.write_all(&bytes[middle..])
        .and_then(|()| file.flush())
        .map_err(|e| PicError::io("write project temporary", Path::new(name), e))?;
    drop(file);
    if name == "manifest.json" {
        fail("manifest_publish")?;
    }
    if replace {
        renameat(dir, temporary.name.as_str(), dir, name)
    } else {
        renameat_with(
            dir,
            temporary.name.as_str(),
            dir,
            name,
            RenameFlags::NOREPLACE,
        )
    }
    .map_err(|e| io("publish project file", Path::new(name), e))?;
    Ok(())
}

// Faults are local to unit tests, never enabled by a CLI environment variable.
fn fail(_stage: &str) -> Result<()> {
    #[cfg(test)]
    if FAILURE.with(|value| value.get() == Some(_stage)) {
        return Err(PicError::new(
            ErrorCode::IoError,
            format!("injected {_stage} failure"),
        ));
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static FAILURE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn with_failure<T>(stage: &'static str, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAILURE.set(None);
        }
    }
    FAILURE.set(Some(stage));
    let _reset = Reset;
    action()
}
