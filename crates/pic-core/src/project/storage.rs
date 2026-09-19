//! Directory-relative, no-follow I/O. Only the manifest publishes history.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{
    AtFlags, CWD, Dir, FlockOperation, Mode, OFlags, RenameFlags, flock, mkdirat, openat, renameat,
    renameat_with, statat, unlinkat,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DerivedKind {
    Checkpoint,
    Preview,
}

impl DerivedKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Checkpoint => "checkpoints",
            Self::Preview => "cache",
        }
    }
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

    fn derived_directory(&self, kind: DerivedKind, create: bool) -> Result<File> {
        let name = kind.directory();
        if create {
            match mkdirat(&self.root, name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                Err(e) => return Err(io("create cache directory", Path::new(name), e)),
            }
        }
        directory(&self.root, Path::new(name))
    }

    // Separate from the history lock: a writer may restore while holding `.lock`.
    fn cache_lock(&self) -> Result<File> {
        let file = File::from(
            openat(
                &self.root,
                ".cache-lock",
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|e| io("open cache lock", &self.path, e))?,
        );
        if !file
            .metadata()
            .map_err(|e| PicError::io("inspect cache lock", &self.path, e))?
            .is_file()
        {
            return Err(PicError::new(
                ErrorCode::UnsafePath,
                "cache lock must be a regular file",
            ));
        }
        flock(&file, FlockOperation::LockExclusive).map_err(|e| io("lock cache", &self.path, e))?;
        Ok(file)
    }

    pub fn derived(&self, kind: DerivedKind, key: &str, limit: u64) -> Result<Vec<u8>> {
        check_hash(key)?;
        read(
            &self.derived_directory(kind, false)?,
            &format!("{key}.bin"),
            limit,
        )
    }

    pub fn derived_length(&self, kind: DerivedKind, key: &str) -> Result<u64> {
        check_hash(key)?;
        let dir = self.derived_directory(kind, false)?;
        open_file(&dir, &format!("{key}.bin"))?
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|e| PicError::io("inspect cache size", &self.path, e))
    }

    /// Eviction only visits digest-named files in the two derived directories.
    /// All cache publishers/clearers serialize here; readers tolerate eviction races.
    pub fn put_derived(
        &self,
        kind: DerivedKind,
        key: &str,
        bytes: &[u8],
        budget: u64,
    ) -> Result<bool> {
        check_hash(key)?;
        let _lock = self.cache_lock()?;
        let directories = [
            self.derived_directory(DerivedKind::Checkpoint, true)?,
            self.derived_directory(DerivedKind::Preview, true)?,
        ];
        let destination = usize::from(kind == DerivedKind::Preview);
        let name = format!("{key}.bin");
        let fits = bytes.len() as u64 <= budget;
        trim_derived(
            &directories,
            budget.saturating_sub(if fits { bytes.len() as u64 } else { 0 }),
            Some((destination, &name)),
        )?;
        if !fits {
            return Ok(false);
        }
        write_atomic(&directories[destination], &name, bytes, true, "cache_write")?;
        Ok(true)
    }

    pub fn clear_derived(&self) -> Result<u64> {
        self.trim_cache(0)
    }

    pub fn trim_cache(&self, budget: u64) -> Result<u64> {
        let _lock = self.cache_lock()?;
        let mut directories = Vec::new();
        for kind in [DerivedKind::Checkpoint, DerivedKind::Preview] {
            match self.derived_directory(kind, false) {
                Ok(dir) => directories.push(dir),
                Err(e) if e.code == ErrorCode::FileNotFound => (),
                Err(e) => return Err(e),
            }
        }
        trim_derived(&directories, budget, None)
    }
}

fn trim_derived(
    directories: &[File],
    budget: u64,
    replacing: Option<(usize, &str)>,
) -> Result<u64> {
    let mut entries = Vec::new();
    let mut total = 0u64;
    for (index, dir) in directories.iter().enumerate() {
        let iter = Dir::read_from(dir).map_err(|e| io("list cache", Path::new("cache"), e))?;
        for entry in iter {
            let entry = entry.map_err(|e| io("list cache entry", Path::new("cache"), e))?;
            let Some(name) = entry.file_name().to_str().ok() else {
                continue;
            };
            if !name
                .strip_suffix(".bin")
                .is_some_and(|key| check_hash(key).is_ok())
            {
                continue;
            }
            let metadata = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|e| io("inspect cache entry", Path::new(name), e))?;
            let bytes = u64::try_from(metadata.st_size).unwrap_or(0);
            total = total.saturating_add(bytes);
            entries.push((
                metadata.st_mtime,
                metadata.st_mtime_nsec,
                index,
                name.to_owned(),
                bytes,
            ));
        }
    }
    // Oldest publication first, with stable tie-breaking. Not an access-time LRU.
    entries.sort();
    let mut removed = 0;
    if let Some((index, name)) = replacing
        && let Some(entry) = entries.iter_mut().find(|e| e.2 == index && e.3 == name)
    {
        unlinkat(&directories[index], name, AtFlags::empty())
            .map_err(|e| io("replace cache entry", Path::new(name), e))?;
        total = total.saturating_sub(entry.4);
        removed += entry.4;
        entry.3.clear();
    }
    for (_, _, index, name, bytes) in entries {
        // Zero also means explicit clear: remove truncated/zero-byte entries too.
        if budget != 0 && total <= budget {
            break;
        }
        if name.is_empty() {
            continue;
        }
        unlinkat(&directories[index], name.as_str(), AtFlags::empty())
            .map_err(|e| io("evict cache entry", Path::new(&name), e))?;
        total = total.saturating_sub(bytes);
        removed += bytes;
    }
    Ok(removed)
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
