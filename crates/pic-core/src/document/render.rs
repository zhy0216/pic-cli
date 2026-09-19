//! Isolated group scopes, bottom-to-top source-over, and explicit lower-sibling clipping.
use super::*;
use std::collections::BTreeMap;

impl Document {
    pub(super) fn render_layers(
        &self,
        target: &TargetId,
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        if let Some(id) = target.0.strip_prefix("mask:") {
            let layer = self.layer(&TargetId(id.into()))?;
            if layer.mask.is_none() {
                return Err(PicError::new(ErrorCode::InvalidTarget, "layer has no mask"));
            }
            // Raw mask inspection ignores visibility/opacity/clips and ancestor masks.
            return self.sample_layer(
                layer,
                self.width,
                self.height,
                self.world_mapping(&layer.id)?,
                true,
                None,
                limits,
            );
        }
        let chosen = if target == &TargetId::canvas() {
            None
        } else {
            self.layer(target)?;
            Some(target)
        };
        self.render_scope(None, self.width, self.height, chosen, limits)
    }

    fn selected_child(
        &self,
        parent: Option<&TargetId>,
        chosen: Option<&TargetId>,
    ) -> Result<Option<TargetId>> {
        let Some(id) = chosen else {
            return Ok(None);
        };
        let mut layer = self.layer(id)?;
        while layer.parent.as_ref() != parent {
            layer = self.layer(layer.parent.as_ref().ok_or_else(|| {
                PicError::new(ErrorCode::InvalidHierarchy, "preview target outside scope")
            })?)?;
        }
        Ok(Some(layer.id.clone()))
    }

    fn render_scope(
        &self,
        parent: Option<&TargetId>,
        width: u32,
        height: u32,
        chosen: Option<&TargetId>,
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        let selected = self.selected_child(parent, chosen)?;
        let siblings: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.parent.as_ref() == parent)
            .collect();
        let referenced: BTreeSet<_> = siblings
            .iter()
            .filter_map(|l| l.clip.as_ref().map(|id| id.0.as_str()))
            .collect();
        let mut coverage: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
        let mut back = Self::solid(width, height, [0.0; 4], limits)?;
        for layer in siblings {
            let selected_here = selected.as_ref() == Some(&layer.id);
            let mut source = layer.clone();
            if layer.visible && matches!(layer.kind, LayerKind::Group) {
                source.raster = self.render_scope(
                    Some(&layer.id),
                    layer.raster.width(),
                    layer.raster.height(),
                    if selected_here && chosen != Some(&layer.id) {
                        chosen
                    } else {
                        None
                    },
                    limits,
                )?;
            }
            let front = if layer.visible {
                self.sample_layer(
                    &source,
                    width,
                    height,
                    layer.mapping(),
                    false,
                    layer.clip.as_ref().map(|id| {
                        coverage
                            .get(id.0.as_str())
                            .expect("validated lower sibling")
                            .as_slice()
                    }),
                    limits,
                )?
            } else {
                Self::solid(width, height, [0.0; 4], limits)?
            };
            if referenced.contains(layer.id.0.as_str()) {
                let mut alpha = Vec::new();
                alpha.try_reserve_exact(front.pixels().len()).map_err(|_| {
                    PicError::new(ErrorCode::ResourceLimit, "cannot allocate clip coverage")
                })?;
                alpha.extend(
                    front
                        .pixels()
                        .iter()
                        .map(|p| (f64::from(p[3]) * layer.opacity) as f32),
                );
                coverage.insert(&layer.id.0, alpha);
            }
            if let LayerKind::Adjustment { params } = &layer.kind {
                if layer.visible && layer.opacity != 0.0 {
                    let adjusted = params.apply(&back, limits)?;
                    let mut pixels = Vec::new();
                    pixels.try_reserve_exact(back.pixels().len()).map_err(|_| {
                        PicError::new(ErrorCode::ResourceLimit, "cannot allocate adjustment scope")
                    })?;
                    for ((old, new), mask) in back
                        .pixels()
                        .iter()
                        .zip(adjusted.pixels())
                        .zip(front.pixels())
                    {
                        let weight = f64::from(mask[3]) * layer.opacity;
                        let mut pixel = *old;
                        if weight == 1.0 {
                            pixel = *new;
                        } else if weight != 0.0 {
                            for c in 0..3 {
                                pixel[c] = (f64::from(old[c]) * (1.0 - weight)
                                    + f64::from(new[c]) * weight)
                                    as f32;
                            }
                        }
                        pixels.push(pixel);
                    }
                    back = Raster::from_linear_rgba(width, height, pixels, limits)?;
                }
            } else {
                if selected_here {
                    back = Self::solid(width, height, [0.0; 4], limits)?;
                }
                let mut pixels = Vec::new();
                pixels.try_reserve_exact(back.pixels().len()).map_err(|_| {
                    PicError::new(ErrorCode::ResourceLimit, "cannot allocate composite scope")
                })?;
                pixels.extend(
                    back.pixels()
                        .iter()
                        .zip(front.pixels())
                        .map(|(b, f)| composite::blend(*b, *f, layer.opacity, layer.blend)),
                );
                back = Raster::from_linear_rgba(width, height, pixels, limits)?;
            }
            // Adjustment inspection returns this scope's adjusted prefix. A raster/text/group
            // inspection returns its isolated contribution, with dependencies and ancestors.
            if selected_here {
                return Ok(back);
            }
        }
        Ok(back)
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_layer(
        &self,
        layer: &Layer,
        width: u32,
        height: u32,
        mapping: Affine,
        mask_only: bool,
        clip: Option<&[f32]>,
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        let count = limits.check_dimensions(width, height)?;
        let inverse = mapping.inverse();
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| {
            PicError::new(
                ErrorCode::ResourceLimit,
                "cannot allocate transformed layer",
            )
        })?;
        for y in 0..height {
            for x in 0..width {
                let mut p = composite::sample(
                    layer,
                    inverse.map([f64::from(x) + 0.5, f64::from(y) + 0.5]),
                    mask_only,
                );
                if let Some(clip) = clip {
                    p[3] *= clip[y as usize * width as usize + x as usize];
                }
                pixels.push(p);
            }
        }
        Raster::from_linear_rgba(width, height, pixels, limits)
    }
}
