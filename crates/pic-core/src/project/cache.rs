//! Disposable accelerators. Keys derive only from authoritative assets and semantic ops.

use std::{cell::RefCell, collections::VecDeque};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    document::{DOCUMENT_SCHEMA_VERSION, Document, PIXEL_SEMANTICS, Raster, RevisionId},
    result::{Diagnostics, Warning, timed},
};

use super::{
    CommittedOp, Project, Result, invalid,
    storage::{self, DerivedKind},
};

// Bump on any decoder, sampling, color, alpha or evaluation change affecting pixels.
// Operation versions and explicit filter parameters are also included in each prefix.
const RENDER_SEMANTICS: &str =
    "pic-render-v1:image-0.25.10:fir-5.5.0:geometry-v1:adjustments-v1:layers-v1";
const MAGIC: &[u8; 8] = b"PICFLT01";
const OVERHEAD: u64 = 120; // magic + key + four dimensions + SHA-256

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

impl ImageSize {
    pub(super) fn of(raster: &Raster) -> Self {
        Self {
            width: raster.width(),
            height: raster.height(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheHit {
    pub kind: &'static str,
    pub tier: &'static str,
    pub key: String,
    pub revision: RevisionId,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ReplayReport {
    pub cache_hit: Option<CacheHit>,
    pub reused_steps: usize,
    /// Actual logical steps executed in order, including newly committed suffixes.
    pub recomputed_revisions: Vec<RevisionId>,
}

#[derive(Debug, Serialize)]
pub struct CheckpointResult {
    pub project: std::path::PathBuf,
    pub revision: RevisionId,
    pub key: String,
    pub canvas: ImageSize,
    pub disk_stored: bool,
    pub memory_stored: bool,
    pub replay: ReplayReport,
}

#[derive(Debug, Serialize)]
pub struct CacheClearResult {
    pub project: std::path::PathBuf,
    pub removed_bytes: u64,
}

#[derive(Clone)]
pub(super) struct CachedRaster {
    pub raster: Raster,
    pub canvas: ImageSize,
    pub document: Option<Document>,
    pub layer_to_canvas: Option<crate::composite::Affine>,
}

impl CachedRaster {
    fn memory_bytes(&self) -> u64 {
        self.raster.pixels().len() as u64 * 16
            + self.document.as_ref().map_or(0, Document::memory_bytes)
            + super::snapshot::metadata(self)
                .ok()
                .flatten()
                .map_or(0, |bytes| bytes.len() as u64)
    }
}

#[derive(Default)]
pub(super) struct MemoryCache {
    entries: VecDeque<(DerivedKind, String, CachedRaster)>,
    bytes: u64,
}

impl MemoryCache {
    fn trim(&mut self, budget: u64) {
        while self.bytes > budget {
            if let Some((_, _, value)) = self.entries.pop_front() {
                self.bytes -= value.memory_bytes();
            }
        }
    }

    fn get(&mut self, kind: DerivedKind, key: &str) -> Option<CachedRaster> {
        let index = self
            .entries
            .iter()
            .position(|e| e.0 == kind && e.1 == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.2.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    fn put(&mut self, kind: DerivedKind, key: &str, value: CachedRaster, budget: u64) -> bool {
        let bytes = value.memory_bytes();
        if bytes > budget {
            return false;
        }
        if let Some(index) = self.entries.iter().position(|e| e.0 == kind && e.1 == key) {
            let (_, _, old) = self.entries.remove(index).expect("existing cache entry");
            self.bytes -= old.memory_bytes();
        }
        self.trim(budget - bytes);
        self.bytes += bytes;
        self.entries.push_back((kind, key.into(), value));
        true
    }
}

impl Project {
    /// IDs/group boundaries deliberately do not enter keys: equivalent semantic prefixes share.
    pub(super) fn prefix_keys(&self, steps: &[&CommittedOp]) -> Result<Vec<String>> {
        let initial = serde_json::json!({
            "cache_schema": 1,
            "document_schema": DOCUMENT_SCHEMA_VERSION,
            "engine": env!("CARGO_PKG_VERSION"),
            "render": RENDER_SEMANTICS,
            "pixel": PIXEL_SEMANTICS,
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
            "source": self.manifest.source,
        });
        let mut keys = vec![json_key(&initial)?];
        for step in steps {
            keys.push(json_key(&serde_json::json!({
                "prefix": keys.last(),
                "operation": step.operation,
                "inputs": step.input_assets,
                "results": step.result_assets,
            }))?);
        }
        Ok(keys)
    }

    /// Required assets are checked even on a memory/preview hit. Cache cannot hide corruption.
    pub(super) fn verified_source(
        &self,
        steps: &[&CommittedOp],
        diagnostics: &mut Diagnostics,
    ) -> Result<Vec<u8>> {
        timed(&mut diagnostics.timings.read_ms, || {
            let source = self
                .storage
                .asset(&self.manifest.source.sha256, self.limits.max_input_bytes)?;
            let verify_length = |asset: &super::AssetRef, bytes: &[u8]| {
                if bytes.len() as u64 != asset.bytes {
                    return Err(crate::PicError::new(
                        crate::ErrorCode::IntegrityMismatch,
                        "asset length does not match reference",
                    ));
                }
                Ok(())
            };
            verify_length(&self.manifest.source, &source)?;
            let mut verified = std::collections::BTreeMap::from([(
                self.manifest.source.sha256.as_str(),
                self.manifest.source.bytes,
            )]);
            for asset in steps
                .iter()
                .flat_map(|s| s.input_assets.iter().chain(&s.result_assets))
            {
                if let Some(length) = verified.get(asset.sha256.as_str()) {
                    if *length != asset.bytes {
                        return Err(invalid("inconsistent dependency length"));
                    }
                } else {
                    let bytes = self
                        .storage
                        .asset(&asset.sha256, self.limits.max_input_bytes)?;
                    verify_length(asset, &bytes)?;
                    verified.insert(&asset.sha256, asset.bytes);
                }
            }
            Ok(source)
        })
    }

    pub(super) fn cache_get(
        &self,
        kind: DerivedKind,
        key: &str,
        diagnostics: &mut Diagnostics,
    ) -> Option<(CachedRaster, &'static str)> {
        let mut memory = self.memory.borrow_mut();
        if let Some(value) = memory.get(kind, key) {
            return Some((value, "memory"));
        }
        // Encoded snapshot + decoded pixels coexist during restore; bound both and retained data.
        let limit = (self.limits.max_cache_memory_bytes / 2)
            .min(self.limits.max_buffer_bytes / 2)
            .min(self.limits.max_cache_disk_bytes);
        if limit < OVERHEAD {
            return None;
        }
        let value = timed(&mut diagnostics.timings.read_ms, || {
            let size = self.storage.derived_length(kind, key).ok()?;
            if size > limit {
                return None;
            }
            memory.trim(self.limits.max_cache_memory_bytes.saturating_sub(size * 2));
            let bytes = self.storage.derived(kind, key, size).ok()?;
            self.decode_snapshot(key, &bytes).ok()
        })?;
        memory.put(kind, key, value.clone(), self.limits.max_cache_memory_bytes);
        Some((value, "disk"))
    }

    pub(super) fn cache_put(
        &self,
        kind: DerivedKind,
        key: &str,
        value: CachedRaster,
        diagnostics: &mut Diagnostics,
    ) -> (bool, bool) {
        let mut memory = self.memory.borrow_mut();
        let metadata = super::snapshot::metadata(&value).ok().flatten();
        let size = metadata
            .as_ref()
            .map_or(value.memory_bytes() + OVERHEAD, |meta| {
                super::snapshot::size(&value, meta)
            });
        let scratch = size.saturating_mul(2);
        let can_serialize = size <= self.limits.max_cache_disk_bytes
            && scratch <= self.limits.max_cache_memory_bytes
            && scratch <= self.limits.max_buffer_bytes;
        let disk = if can_serialize {
            memory.trim(self.limits.max_cache_memory_bytes - scratch);
            timed(&mut diagnostics.timings.write_ms, || {
                let bytes = encode_snapshot(key, &value);
                self.storage
                    .put_derived(kind, key, &bytes, self.limits.max_cache_disk_bytes)
            })
        } else {
            timed(&mut diagnostics.timings.write_ms, || {
                self.storage
                    .trim_cache(self.limits.max_cache_disk_bytes)
                    .map(|_| false)
            })
        };
        let disk = match disk {
            Ok(stored) => stored,
            Err(_) => {
                diagnostics.warnings.push(Warning { code: "cache_write_skipped", message: "Derived cache could not be stored; authoritative assets and history remain usable." });
                false
            }
        };
        let retained = memory.put(kind, key, value, self.limits.max_cache_memory_bytes);
        (disk, retained)
    }

    fn decode_snapshot(&self, key: &str, bytes: &[u8]) -> Result<CachedRaster> {
        if bytes.len() < OVERHEAD as usize
            || (&bytes[..8] != MAGIC && &bytes[..8] != super::snapshot::MAGIC)
            || &bytes[8..72] != key.as_bytes()
            || Sha256::digest(&bytes[..bytes.len() - 32]).as_slice() != &bytes[bytes.len() - 32..]
        {
            return Err(invalid("invalid derived snapshot header or checksum"));
        }
        if &bytes[..8] == super::snapshot::MAGIC {
            return super::snapshot::decode(&bytes[..bytes.len() - 32], &self.limits);
        }
        let dimension = |offset| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("checked header"),
            )
        };
        let canvas = ImageSize {
            width: dimension(72),
            height: dimension(76),
        };
        self.limits.check_dimensions(canvas.width, canvas.height)?;
        let (width, height) = (dimension(80), dimension(84));
        let count = self.limits.check_dimensions(width, height)?;
        if bytes.len() as u64 != count as u64 * 16 + OVERHEAD {
            return Err(invalid("invalid derived snapshot length"));
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| invalid("cannot allocate snapshot pixels"))?;
        for pixel in bytes[88..bytes.len() - 32].as_chunks::<16>().0 {
            pixels.push(std::array::from_fn(|channel| {
                f32::from_le_bytes(
                    pixel[channel * 4..channel * 4 + 4]
                        .try_into()
                        .expect("four bytes"),
                )
            }));
        }
        Ok(CachedRaster {
            raster: Raster::from_linear_rgba(width, height, pixels, &self.limits)?,
            canvas,
            document: None,
            layer_to_canvas: None,
        })
    }

    pub fn checkpoint(
        &self,
        revision: Option<&str>,
        diagnostics: &mut Diagnostics,
    ) -> Result<CheckpointResult> {
        let restored = self.restore(revision, diagnostics)?;
        let steps = self.history.path(&restored.revision.0)?;
        let keys = self.prefix_keys(&steps)?;
        let key = keys.last().expect("source key");
        let canvas = ImageSize::of(&restored.raster);
        let (disk_stored, memory_stored) = self.cache_put(
            DerivedKind::Checkpoint,
            key,
            CachedRaster {
                raster: restored.raster,
                canvas,
                document: restored.document.layered.then_some(restored.document),
                layer_to_canvas: None,
            },
            diagnostics,
        );
        Ok(CheckpointResult {
            project: self.path().to_owned(),
            revision: restored.revision,
            key: key.clone(),
            canvas,
            disk_stored,
            memory_stored,
            replay: restored.replay,
        })
    }

    pub fn clear_cache(&self, diagnostics: &mut Diagnostics) -> Result<CacheClearResult> {
        let removed_bytes = timed(&mut diagnostics.timings.write_ms, || {
            self.storage.clear_derived()
        })?;
        *self.memory.borrow_mut() = MemoryCache::default();
        Ok(CacheClearResult {
            project: self.path().to_owned(),
            removed_bytes,
        })
    }
}

pub(super) fn json_key(value: &impl Serialize) -> Result<String> {
    // Value uses sorted object keys, including arbitrarily nested operation parameters.
    let canonical = serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?;
    Ok(storage::hash(
        &serde_json::to_vec(&canonical).map_err(|e| invalid(e.to_string()))?,
    ))
}

fn encode_snapshot(key: &str, value: &CachedRaster) -> Vec<u8> {
    if let Some(metadata) = super::snapshot::metadata(value).expect("validated finite metadata") {
        let mut bytes = super::snapshot::encode(key, value, &metadata);
        let checksum = Sha256::digest(&bytes);
        bytes.extend_from_slice(&checksum);
        return bytes;
    }
    let mut bytes = Vec::with_capacity((value.memory_bytes() + OVERHEAD) as usize);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(key.as_bytes());
    for dimension in [
        value.canvas.width,
        value.canvas.height,
        value.raster.width(),
        value.raster.height(),
    ] {
        bytes.extend_from_slice(&dimension.to_le_bytes());
    }
    for pixel in value.raster.pixels() {
        for channel in pixel {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
    }
    let checksum = Sha256::digest(&bytes);
    bytes.extend_from_slice(&checksum);
    bytes
}

pub(super) fn new_memory() -> RefCell<MemoryCache> {
    RefCell::new(MemoryCache::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::ResourceLimits;

    #[test]
    fn snapshot_codec_preserves_signed_zero_subnormals_hdr_and_hidden_rgb_bits() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]))
            .save(&input)
            .unwrap();
        let path = dir.path().join("test.pic");
        let limits = ResourceLimits::default();
        Project::create(&input, &path, &limits, &mut Diagnostics::default()).unwrap();
        let project = Project::open(&path, &limits, &mut Diagnostics::default()).unwrap();
        let samples = vec![
            [-0.0, f32::from_bits(1), f32::MAX, 0.0],
            [-f32::MAX, 1.2345678, -f32::from_bits(1), 0.12345679],
        ];
        let value = CachedRaster {
            raster: Raster::from_linear_rgba(2, 1, samples.clone(), &limits).unwrap(),
            document: None,
            layer_to_canvas: None,
            canvas: ImageSize {
                width: 2,
                height: 1,
            },
        };
        let key = storage::hash(b"codec test");
        let bytes = encode_snapshot(&key, &value);
        let decoded = project.decode_snapshot(&key, &bytes).unwrap();
        for (a, b) in samples.iter().zip(decoded.raster.pixels()) {
            assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
        }
        // Even a checksum-valid file with an impossible working alpha is rejected.
        let mut invalid = bytes;
        invalid[100..104].copy_from_slice(&2.0f32.to_le_bytes());
        let payload = invalid.len() - 32;
        let checksum = Sha256::digest(&invalid[..payload]);
        invalid[payload..].copy_from_slice(&checksum);
        assert!(project.decode_snapshot(&key, &invalid).is_err());
    }
}
