use super::*;
use std::collections::{BTreeMap, VecDeque};

pub const MAX_GROUP_DEPTH: usize = 32;

impl Document {
    pub(crate) fn solid(
        width: u32,
        height: u32,
        color: [f32; 4],
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        let count = limits.check_dimensions(width, height)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| PicError::new(ErrorCode::ResourceLimit, "cannot allocate layer buffer"))?;
        pixels.resize(count, color);
        Raster::from_linear_rgba(width, height, pixels, limits)
    }

    pub(crate) fn validate_hierarchy(&self, limits: &ResourceLimits) -> Result<()> {
        let indices: BTreeMap<_, _> = self
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| (l.id.0.as_str(), i))
            .collect();
        let index = |id: &TargetId| {
            indices.get(id.0.as_str()).copied().ok_or_else(|| {
                PicError::new(
                    ErrorCode::InvalidTarget,
                    format!("dependency '{}' does not exist", id.0),
                )
            })
        };
        let mut edges = vec![Vec::new(); self.layers.len()];
        let mut incoming = vec![0; self.layers.len()];
        // Render dependencies: a group reads its children; a clipped layer reads its base.
        for (i, layer) in self.layers.iter().enumerate() {
            if let Some(parent) = &layer.parent {
                let p = index(parent)?;
                if !matches!(self.layers[p].kind, LayerKind::Group) {
                    return Err(PicError::new(
                        ErrorCode::InvalidHierarchy,
                        "a parent must be a group",
                    ));
                }
                edges[p].push(i);
                incoming[i] += 1;
            }
            if let Some(base) = &layer.clip {
                let b = index(base)?;
                edges[i].push(b);
                incoming[b] += 1;
            }
        }
        let mut ready: VecDeque<_> = incoming
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (*n == 0).then_some(i))
            .collect();
        let mut visited = 0;
        while let Some(i) = ready.pop_front() {
            visited += 1;
            for &next in &edges[i] {
                incoming[next] -= 1;
                if incoming[next] == 0 {
                    ready.push_back(next);
                }
            }
        }
        if visited != self.layers.len() {
            return Err(PicError::new(
                ErrorCode::DependencyCycle,
                "group/clipping dependency cycle",
            ));
        }
        for (i, layer) in self.layers.iter().enumerate() {
            let mut depth = 0;
            let mut current = layer;
            while let Some(parent) = &current.parent {
                depth += 1;
                if depth > MAX_GROUP_DEPTH {
                    return Err(PicError::new(
                        ErrorCode::ResourceLimit,
                        "group nesting exceeds 32 levels",
                    ));
                }
                current = &self.layers[index(parent)?];
            }
            self.world_mapping(&layer.id)?;
            if let Some(base) = &layer.clip {
                let b = index(base)?;
                if b >= i
                    || self.layers[b].parent != layer.parent
                    || matches!(self.layers[b].kind, LayerKind::Adjustment { .. })
                {
                    return Err(PicError::new(
                        ErrorCode::InvalidHierarchy,
                        "clip base must be a lower sibling raster, text or group; adjustment layers cannot supply independent alpha",
                    ));
                }
            }
        }
        // Shared immutable input pixels are counted once, independently of scope scratch.
        // This bounds admitted pixel buffers, not total process RSS or codec/font internals.
        if self.layered
            && self
                .memory_bytes()
                .saturating_add(self.scope_scratch(None, self.width, self.height))
                > limits.max_buffer_bytes
        {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "group/clip/adjustment render buffers exceed memory budget",
            ));
        }
        Ok(())
    }

    fn scope_scratch(&self, parent: Option<&TargetId>, width: u32, height: u32) -> u64 {
        let area = u64::from(width) * u64::from(height);
        let siblings: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.parent.as_ref() == parent)
            .collect();
        let bases: BTreeSet<_> = siblings
            .iter()
            .filter_map(|l| l.clip.as_ref().map(|id| &id.0))
            .collect();
        let coverage = area.saturating_mul(4).saturating_mul(bases.len() as u64);
        // Ordinary composition: current back + sampled front + next back (3 RGBA32F).
        // Adjustment interpolation also retains the adjusted back (4 RGBA32F).
        let planes = if siblings
            .iter()
            .any(|l| l.visible && matches!(l.kind, LayerKind::Adjustment { .. }))
        {
            64
        } else {
            48
        };
        let mut peak = area.saturating_mul(planes).saturating_add(coverage);
        for group in siblings
            .into_iter()
            .filter(|l| l.visible && matches!(l.kind, LayerKind::Group))
        {
            let (w, h) = (group.raster.width(), group.raster.height());
            // During recursion only this scope's back and clip coverage remain live.
            let recursive = area
                .saturating_mul(16)
                .saturating_add(coverage)
                .saturating_add(self.scope_scratch(Some(&group.id), w, h));
            // After recursion the group's offscreen raster remains live while its parent
            // samples/composites it; other completed group rasters have already been freed.
            let composite = area
                .saturating_mul(48)
                .saturating_add(coverage)
                .saturating_add(u64::from(w) * u64::from(h) * 16);
            peak = peak.max(recursive).max(composite);
        }
        peak
    }

    pub fn world_mapping(&self, id: &TargetId) -> Result<Affine> {
        let mut layer = self.layer(id)?;
        let mut mapping = layer.mapping();
        let mut depth = 0;
        while let Some(parent) = &layer.parent {
            depth += 1;
            if depth > MAX_GROUP_DEPTH {
                return Err(PicError::new(
                    ErrorCode::InvalidHierarchy,
                    "cyclic or excessively deep group hierarchy",
                ));
            }
            layer = self.layer(parent)?;
            mapping = layer.mapping().then(mapping);
        }
        if mapping.checked_inverse().is_none() {
            return Err(PicError::new(
                ErrorCode::InvalidHierarchy,
                "composed group transform has no reliable finite inverse (determinant overflow, underflow or precision loss)",
            ));
        }
        Ok(mapping)
    }
}
