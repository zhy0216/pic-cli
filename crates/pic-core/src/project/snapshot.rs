//! Extended disposable snapshot: JSON editing metadata + exact little-endian f32 rasters.
use super::{ImageSize, cache::CachedRaster, invalid};
use crate::{
    Result,
    composite::Affine,
    document::{Document, DocumentInfo, Layer, Raster},
    limits::ResourceLimits,
};
use serde::{Deserialize, Serialize};

pub(super) const MAGIC: &[u8; 8] = b"PICDOC03";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    canvas: ImageSize,
    document: Option<DocumentInfo>,
    layer_to_canvas: Option<Affine>,
}

pub(super) fn metadata(value: &CachedRaster) -> Result<Option<Vec<u8>>> {
    if value.document.is_none() && value.layer_to_canvas.is_none() {
        return Ok(None);
    }
    serde_json::to_vec(&Metadata {
        canvas: value.canvas,
        document: value.document.as_ref().map(Document::info),
        layer_to_canvas: value.layer_to_canvas,
    })
    .map(Some)
    .map_err(|e| invalid(e.to_string()))
}
fn rasters(value: &CachedRaster) -> impl Iterator<Item = &Raster> {
    std::iter::once(&value.raster).chain(value.document.iter().flat_map(|doc| {
        doc.layers
            .iter()
            .flat_map(|layer| std::iter::once(&layer.raster).chain(layer.mask.as_ref()))
    }))
}
pub(super) fn size(value: &CachedRaster, metadata: &[u8]) -> u64 {
    112 + metadata.len() as u64
        + rasters(value)
            .map(|r| 8 + r.pixels().len() as u64 * 16)
            .sum::<u64>()
}
pub(super) fn encode(key: &str, value: &CachedRaster, metadata: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size(value, metadata) as usize);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&(metadata.len() as u64).to_le_bytes());
    bytes.extend_from_slice(metadata);
    for raster in rasters(value) {
        bytes.extend_from_slice(&raster.width().to_le_bytes());
        bytes.extend_from_slice(&raster.height().to_le_bytes());
        for p in raster.pixels() {
            for c in p {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    bytes
}
pub(super) fn decode(bytes: &[u8], limits: &ResourceLimits) -> Result<CachedRaster> {
    let length = u64::from_le_bytes(
        bytes
            .get(72..80)
            .ok_or_else(|| invalid("truncated document snapshot"))?
            .try_into()
            .expect("eight bytes"),
    );
    if length > limits.max_project_bytes {
        return Err(invalid("snapshot metadata exceeds budget"));
    }
    let mut reader = Reader { bytes, offset: 80 };
    let meta: Metadata = serde_json::from_slice(
        reader.take(usize::try_from(length).map_err(|_| invalid("snapshot length overflow"))?)?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    limits.check_dimensions(meta.canvas.width, meta.canvas.height)?;
    if let Some(matrix) = meta.layer_to_canvas
        && matrix.checked_inverse().is_none()
    {
        return Err(invalid("invalid snapshot coordinate mapping"));
    }
    let raster = reader.raster(limits)?;
    let document = meta
        .document
        .map(|info| {
            // One initial layer plus at most one new ID per operation; usize::MAX stays unbounded.
            let max_layer_ids = limits.max_history_operations.saturating_add(1);
            if !info.layered
                || info.width != meta.canvas.width
                || info.height != meta.canvas.height
                || info.layers.len() > max_layer_ids
                || info.used_layer_ids.len() > max_layer_ids
            {
                return Err(invalid("invalid document snapshot metadata"));
            }
            let mut layers = Vec::new();
            let mut mappings = Vec::new();
            for layer in info.layers {
                let raster = reader.raster(limits)?;
                if raster.width() != layer.width || raster.height() != layer.height {
                    return Err(invalid("snapshot layer dimensions differ"));
                }
                let mask = if let Some(id) = layer.mask {
                    if id.0 != format!("mask:{}", layer.id.0) {
                        return Err(invalid("invalid mask identity"));
                    }
                    Some(reader.raster(limits)?)
                } else {
                    None
                };
                let rebuilt = Layer {
                    kind: layer.kind,
                    parent: layer.parent,
                    clip: layer.clip,
                    id: layer.id,
                    name: layer.name,
                    visible: layer.visible,
                    opacity: layer.opacity,
                    blend: layer.blend,
                    transform: layer.transform,
                    raster,
                    mask,
                };
                mappings.push((layer.layer_to_canvas, layer.canvas_to_layer));
                layers.push(rebuilt);
            }
            let doc = Document {
                width: info.width,
                height: info.height,
                layers,
                selection: info.selection,
                layered: info.layered,
                used_layer_ids: info.used_layer_ids,
            };
            doc.validate(limits)?;
            for (layer, (forward, inverse)) in doc.layers.iter().zip(mappings) {
                let actual = doc.world_mapping(&layer.id)?;
                if actual != forward || actual.inverse() != inverse {
                    return Err(invalid("snapshot mapping differs from group transforms"));
                }
            }
            Ok(doc)
        })
        .transpose()?;
    if reader.offset != bytes.len() {
        return Err(invalid("unexpected document snapshot payload"));
    }
    Ok(CachedRaster {
        raster,
        canvas: meta.canvas,
        document,
        layer_to_canvas: meta.layer_to_canvas,
    })
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| invalid("snapshot length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("truncated snapshot payload"))?;
        self.offset = end;
        Ok(value)
    }
    fn raster(&mut self, limits: &ResourceLimits) -> Result<Raster> {
        let width = u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes"));
        let height = u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes"));
        let count = limits.check_dimensions(width, height)?;
        let payload = self.take(
            count
                .checked_mul(16)
                .ok_or_else(|| invalid("snapshot raster overflow"))?,
        )?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| invalid("cannot allocate snapshot pixels"))?;
        for p in payload.as_chunks::<16>().0 {
            pixels.push(std::array::from_fn(|c| {
                f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().expect("four bytes"))
            }));
        }
        Raster::from_linear_rgba(width, height, pixels, limits)
    }
}
