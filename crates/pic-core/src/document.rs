//! Shared pixel state and identities. Persistent assets and ops live in `project`.
mod hierarchy;
mod render;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{ErrorCode, PicError, Result, limits::ResourceLimits};

pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const PIXEL_SEMANTICS: &str = "linear_srgb_rgba32f_straight_v1";

/// Opaque identity, never a display name or an index. Stateless images use `canvas`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetId(pub String);

impl TargetId {
    pub fn canvas() -> Self {
        Self("canvas".into())
    }
}

/// An immutable state identifier, independent of the document schema version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub String);

/// Pixels are linear-light sRGB, straight (unassociated) alpha, in row-major RGBA order.
/// RGB may be finite HDR/negative values; alpha is finite and in [0, 1].
/// Clones share immutable samples. Checkpoints preserve every f32 bit.
#[derive(Debug, Clone)]
pub struct Raster {
    width: u32,
    height: u32,
    pixels: Arc<Vec<[f32; 4]>>,
}

impl Raster {
    pub fn from_linear_rgba(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        limits: &ResourceLimits,
    ) -> Result<Self> {
        let count = limits.check_dimensions(width, height)?;
        if pixels.len() != count {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "pixel count does not match dimensions",
            ));
        }
        if pixels
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&p[3]))
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "pixels must be finite and alpha must be in [0, 1]",
            ));
        }
        Ok(Self {
            width,
            height,
            pixels: Arc::new(pixels),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[[f32; 4]] {
        self.pixels.as_slice()
    }
    pub fn has_transparency(&self) -> bool {
        self.pixels.iter().any(|pixel| pixel[3] < 1.0)
    }
}

use crate::{
    composite::{self, Affine, BlendMode, Transform},
    operation::{
        Operation,
        geometry::Anchor,
        layers::{LayerOperation, valid_id},
    },
};
use std::collections::BTreeSet;

pub const BASE_LAYER_ID: &str = "base";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayerKind {
    #[default]
    Raster,
    Group,
    Text {
        params: crate::text::TextParams,
    },
    Adjustment {
        params: crate::operation::layers::AdjustmentParams,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Region {
    fn contains(&self, [x, y]: [f64; 2]) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// Union of axis-aligned rectangles in the named canvas or layer's pixel-edge space.
/// An empty union selects nothing; None selects all. This is editing state, not render clipping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub space: TargetId,
    pub regions: Vec<Region>,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        if self.space != TargetId::canvas() {
            valid_id(&self.space)?;
        }
        if self.regions.len() > 1024
            || self.regions.iter().any(|r| {
                ![r.x, r.y, r.width, r.height, r.x + r.width, r.y + r.height]
                    .iter()
                    .all(|v| v.is_finite())
                    || r.width <= 0.0
                    || r.height <= 0.0
            })
        {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "selection requires at most 1024 finite, positive rectangles",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub id: TargetId,
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    pub blend: BlendMode,
    pub transform: Transform,
    pub raster: Raster,
    /// Coverage is in alpha, RGB is zero. Same local dimensions as raster.
    pub mask: Option<Raster>,
    pub kind: LayerKind,
    pub parent: Option<TargetId>,
    pub clip: Option<TargetId>,
}
impl Layer {
    pub fn new(id: TargetId, name: String, raster: Raster) -> Self {
        Self {
            id,
            name,
            raster,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            transform: Transform::default(),
            mask: None,
            kind: LayerKind::Raster,
            parent: None,
            clip: None,
        }
    }
    pub fn mapping(&self) -> Affine {
        self.transform
            .matrix(self.raster.width(), self.raster.height())
    }
    pub fn info(&self) -> LayerInfo {
        LayerInfo {
            kind: self.kind.clone(),
            parent: self.parent.clone(),
            clip: self.clip.clone(),
            id: self.id.clone(),
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
            blend: self.blend,
            transform: self.transform.clone(),
            width: self.raster.width(),
            height: self.raster.height(),
            mask: self
                .mask
                .as_ref()
                .map(|_| TargetId(format!("mask:{}", self.id.0))),
            layer_to_canvas: self.mapping(),
            canvas_to_layer: self.mapping().inverse(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerInfo {
    pub kind: LayerKind,
    pub parent: Option<TargetId>,
    pub clip: Option<TargetId>,
    pub id: TargetId,
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    pub blend: BlendMode,
    pub transform: Transform,
    pub width: u32,
    pub height: u32,
    pub mask: Option<TargetId>,
    pub layer_to_canvas: Affine,
    pub canvas_to_layer: Affine,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentInfo {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<LayerInfo>,
    pub selection: Option<Selection>,
    /// Legacy canvas operations retain their original single-raster behavior until a layer/selection edit.
    pub layered: bool,
    pub used_layer_ids: BTreeSet<String>,
}

/// The sole evaluated editing state. Assets + immutable ops reconstruct it; snapshots only cache it.
#[derive(Debug, Clone)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub selection: Option<Selection>,
    pub(crate) layered: bool,
    pub(crate) used_layer_ids: BTreeSet<String>,
}
impl Document {
    pub fn from_raster(raster: Raster) -> Self {
        Self {
            width: raster.width(),
            height: raster.height(),
            layers: vec![Layer::new(
                TargetId(BASE_LAYER_ID.into()),
                "Base".into(),
                raster,
            )],
            selection: None,
            layered: false,
            used_layer_ids: BTreeSet::from([BASE_LAYER_ID.into()]),
        }
    }
    pub fn info(&self) -> DocumentInfo {
        DocumentInfo {
            width: self.width,
            height: self.height,
            layers: self
                .layers
                .iter()
                .map(|layer| {
                    let mut info = layer.info();
                    if let Ok(mapping) = self.world_mapping(&layer.id) {
                        info.layer_to_canvas = mapping;
                        info.canvas_to_layer = mapping.inverse();
                    }
                    info
                })
                .collect(),
            selection: self.selection.clone(),
            layered: self.layered,
            used_layer_ids: self.used_layer_ids.clone(),
        }
    }
    pub fn layer(&self, id: &TargetId) -> Result<&Layer> {
        Ok(&self.layers[self.index(id)?])
    }
    pub(crate) fn index(&self, id: &TargetId) -> Result<usize> {
        self.layers.iter().position(|l| &l.id == id).ok_or_else(|| {
            PicError::new(
                ErrorCode::InvalidTarget,
                format!("layer '{}' does not exist at this revision", id.0),
            )
        })
    }
    pub fn memory_bytes(&self) -> u64 {
        self.layers
            .iter()
            .map(|l| {
                (l.raster.pixels().len() as u64
                    + l.mask.as_ref().map_or(0, |m| m.pixels().len() as u64))
                    * 16
            })
            .sum()
    }
    pub fn validate(&self, limits: &ResourceLimits) -> Result<()> {
        limits.check_dimensions(self.width, self.height)?;
        if self.layered
            && self
                .memory_bytes()
                .saturating_add(u64::from(self.width) * u64::from(self.height) * 32)
                > limits.max_buffer_bytes
        {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "document layers, masks and render buffers exceed memory budget",
            ));
        }
        let mut ids = BTreeSet::new();
        for layer in &self.layers {
            valid_id(&layer.id)?;
            if !ids.insert(&layer.id.0) || !self.used_layer_ids.contains(&layer.id.0) {
                return Err(PicError::new(
                    ErrorCode::InvalidTarget,
                    "duplicate or unregistered layer ID",
                ));
            }
            limits.check_dimensions(layer.raster.width(), layer.raster.height())?;
            layer.transform.validate()?;
            crate::operation::layers::opacity(layer.opacity)?;
            if let Some(mask) = &layer.mask {
                Self::check_mask(&layer.raster, mask)?;
            }
            match &layer.kind {
                LayerKind::Text { params } => {
                    params.validate()?;
                    if params.width != layer.raster.width()
                        || params.height != layer.raster.height()
                    {
                        return Err(PicError::new(
                            ErrorCode::InvalidProject,
                            "text box and raster dimensions differ",
                        ));
                    }
                }
                LayerKind::Adjustment { params } => {
                    params.operation()?;
                    if layer.blend != BlendMode::Normal {
                        return Err(PicError::new(
                            ErrorCode::InvalidArgument,
                            "adjustment layers require normal blend; opacity controls interpolation",
                        ));
                    }
                }
                _ => (),
            }
        }
        self.validate_hierarchy(limits)?;
        if let Some(selection) = &self.selection {
            selection.validate()?;
            if selection.space != TargetId::canvas() {
                self.layer(&selection.space)?;
            }
        }
        if !self.layered
            && (self.layers.len() != 1
                || self.layers[0].id.0 != BASE_LAYER_ID
                || self.layers[0].transform != Transform::default()
                || self.layers[0].mask.is_some()
                || !self.layers[0].visible
                || self.layers[0].opacity != 1.0
                || self.layers[0].blend != BlendMode::Normal
                || !matches!(self.layers[0].kind, LayerKind::Raster)
                || self.layers[0].parent.is_some()
                || self.layers[0].clip.is_some()
                || self.layers[0].raster.width() != self.width
                || self.layers[0].raster.height() != self.height
                || self.selection.is_some())
        {
            return Err(PicError::new(
                ErrorCode::InvalidProject,
                "invalid legacy canvas state",
            ));
        }
        Ok(())
    }
    pub(crate) fn check_mask(raster: &Raster, mask: &Raster) -> Result<()> {
        if (raster.width(), raster.height()) != (mask.width(), mask.height()) {
            return Err(PicError::new(
                ErrorCode::InvalidMask,
                "mask dimensions must match the layer's local pixel dimensions after EXIF normalization",
            ));
        }
        Ok(())
    }
    pub fn render(&self, target: &TargetId, limits: &ResourceLimits) -> Result<Raster> {
        let raster = self.render_for_preview(target, limits)?;
        if target.0.starts_with("mask:") {
            composite::visualize_coverage(&raster, limits)
        } else {
            Ok(raster)
        }
    }

    /// Coverage stays in alpha until all observation sampling is finished. Converting it to
    /// display gray before resizing would incorrectly apply the display transfer function.
    pub(crate) fn render_for_preview(
        &self,
        target: &TargetId,
        limits: &ResourceLimits,
    ) -> Result<Raster> {
        self.validate(limits)?;
        if target == &TargetId::canvas() && !self.layered {
            return Ok(self.layers[0].raster.clone());
        }
        self.render_layers(target, limits)
    }
    pub fn target_mapping(&self, target: &TargetId) -> Result<Option<Affine>> {
        if target == &TargetId::canvas() {
            return Ok(None);
        }
        let id = TargetId(target.0.strip_prefix("mask:").unwrap_or(&target.0).into());
        let layer = self.layer(&id)?;
        if target.0.starts_with("mask:") && layer.mask.is_none() {
            return Err(PicError::new(ErrorCode::InvalidTarget, "layer has no mask"));
        }
        Ok(Some(self.world_mapping(&layer.id)?))
    }

    /// Execute a logical operation. Callers own/discard the state on failure before publication.
    pub(crate) fn apply(
        &mut self,
        target: &TargetId,
        op: &Operation,
        resources: &crate::pipeline::ResourceResolver,
        limits: &ResourceLimits,
        load: &mut impl FnMut(&str, bool) -> Result<Raster>,
        load_font: &mut impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<()> {
        match op {
            Operation::Layer(action) => {
                self.apply_layer(target, action, limits, load, load_font)?
            }
            _ if target == &TargetId::canvas() && !self.layered => {
                Self::apply_raster(&mut self.layers[0].raster, op, resources, limits, load)?;
                self.width = self.layers[0].raster.width();
                self.height = self.layers[0].raster.height();
            }
            Operation::Identity if target == &TargetId::canvas() => (),
            Operation::Crop(p) if target == &TargetId::canvas() => {
                if p.x + p.width > self.width || p.y + p.height > self.height {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "crop rectangle must lie inside canvas",
                    ));
                }
                self.translate_canvas(-f64::from(p.x), -f64::from(p.y));
                self.width = p.width;
                self.height = p.height;
            }
            Operation::Canvas(p) if target == &TargetId::canvas() => {
                if p.background.0 != [0, 0, 0, 0] {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "layered canvas padding requires transparent black; use a background layer for color",
                    ));
                }
                let dx = i64::from(p.width) - i64::from(self.width);
                let dy = i64::from(p.height) - i64::from(self.height);
                let x = match p.anchor {
                    Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 0,
                    Anchor::Top | Anchor::Center | Anchor::Bottom => dx.div_euclid(2),
                    _ => dx,
                };
                let y = match p.anchor {
                    Anchor::TopLeft | Anchor::Top | Anchor::TopRight => 0,
                    Anchor::Left | Anchor::Center | Anchor::Right => dy.div_euclid(2),
                    _ => dy,
                };
                self.translate_canvas(x as f64, y as f64);
                self.width = p.width;
                self.height = p.height;
            }
            _ if target == &TargetId::canvas() => {
                return Err(PicError::new(
                    ErrorCode::InvalidTarget,
                    "layered canvas supports identity/crop/transparent canvas only; select a stable layer ID for pixel edits, or use layer_transform",
                ));
            }
            _ => {
                let i = self.index(target)?;
                if !matches!(self.layers[i].kind, LayerKind::Raster) {
                    return Err(PicError::new(
                        ErrorCode::InvalidTarget,
                        "local pixel edits require a raster layer; use text_set, adjustment_set or layer_transform for editable generated layers",
                    ));
                }
                let geometry = matches!(
                    op,
                    Operation::Crop(_)
                        | Operation::Resize(_)
                        | Operation::Rotate(_)
                        | Operation::Flip(_)
                        | Operation::Canvas(_)
                );
                if geometry && (self.layers[i].mask.is_some() || self.selection.is_some()) {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "pixel geometry with a mask/selection is ambiguous; use layer_transform, or remove the mask and clear the selection first",
                    ));
                }
                if matches!(op, Operation::Composite(_)) && self.selection.is_some() {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "clear selection before composite",
                    ));
                }
                let mut edited = self.layers[i].raster.clone();
                Self::apply_raster(&mut edited, op, resources, limits, load)?;
                if let Some(selection) = &self.selection {
                    let mapping = if selection.space == TargetId::canvas() {
                        self.world_mapping(&self.layers[i].id)?
                    } else {
                        self.world_mapping(&selection.space)?
                            .inverse()
                            .then(self.world_mapping(&self.layers[i].id)?)
                    };
                    let old = &self.layers[i].raster;
                    let pixels = edited
                        .pixels()
                        .iter()
                        .enumerate()
                        .map(|(index, p)| {
                            let point = mapping.map([
                                (index % old.width() as usize) as f64 + 0.5,
                                (index / old.width() as usize) as f64 + 0.5,
                            ]);
                            if selection.regions.iter().any(|r| r.contains(point)) {
                                *p
                            } else {
                                old.pixels()[index]
                            }
                        })
                        .collect();
                    edited =
                        Raster::from_linear_rgba(edited.width(), edited.height(), pixels, limits)?;
                }
                self.layers[i].raster = edited;
                self.layered = true;
            }
        }
        self.validate(limits)
    }
    fn translate_canvas(&mut self, dx: f64, dy: f64) {
        for layer in &mut self.layers {
            if layer.parent.is_some() {
                continue;
            }
            layer.transform.x += dx;
            layer.transform.y += dy;
        }
        if let Some(s) = &mut self.selection
            && s.space == TargetId::canvas()
        {
            for r in &mut s.regions {
                r.x += dx;
                r.y += dy;
            }
        }
    }
    fn apply_raster(
        raster: &mut Raster,
        op: &Operation,
        resources: &crate::pipeline::ResourceResolver,
        limits: &ResourceLimits,
        load: &mut impl FnMut(&str, bool) -> Result<Raster>,
    ) -> Result<()> {
        if let Operation::Composite(p) = op {
            let mut doc = Self::from_raster(raster.clone());
            let mut layer = Layer::new(
                TargetId("composite".into()),
                String::new(),
                load(&p.source, false)?,
            );
            layer.mask = p.mask.as_ref().map(|s| load(s, true)).transpose()?;
            layer.opacity = p.opacity;
            layer.blend = p.blend;
            layer.transform = p.transform.clone();
            doc.layers.push(layer);
            doc.used_layer_ids.insert("composite".into());
            doc.layered = true;
            *raster = doc.render(&TargetId::canvas(), limits)?;
            Ok(())
        } else {
            op.apply_with_limits(raster, resources, limits)
        }
    }
    fn apply_layer(
        &mut self,
        target: &TargetId,
        op: &LayerOperation,
        limits: &ResourceLimits,
        load: &mut impl FnMut(&str, bool) -> Result<Raster>,
        load_font: &mut impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<()> {
        match op {
            LayerOperation::GroupAdd(p) => {
                let mut layer = Layer::new(
                    p.id.clone(),
                    p.name.clone(),
                    Self::solid(p.width, p.height, [0.0; 4], limits)?,
                );
                layer.kind = LayerKind::Group;
                self.insert_layer(layer)?;
            }
            LayerOperation::TextAdd(p) => {
                let raster = crate::text::render(&p.text, &load_font(&p.text.font)?, limits)?;
                let mut layer = Layer::new(p.id.clone(), p.name.clone(), raster);
                layer.kind = LayerKind::Text {
                    params: p.text.clone(),
                };
                self.insert_layer(layer)?;
            }
            LayerOperation::AdjustmentAdd(p) => {
                let mut layer = Layer::new(
                    p.id.clone(),
                    p.name.clone(),
                    Self::solid(p.width, p.height, [0.0, 0.0, 0.0, 1.0], limits)?,
                );
                layer.kind = LayerKind::Adjustment {
                    params: p.adjustment.clone(),
                };
                self.insert_layer(layer)?;
            }
            LayerOperation::Add(p) => {
                if self.used_layer_ids.contains(&p.id.0) {
                    return Err(PicError::new(
                        ErrorCode::InvalidTarget,
                        "layer ID has already been used in this history",
                    ));
                }
                let layer = Layer::new(p.id.clone(), p.name.clone(), load(&p.source, false)?);
                self.used_layer_ids.insert(p.id.0.clone());
                self.layers.push(layer);
            }
            LayerOperation::SelectionSet(p) => {
                let (w, h) = if p.space == TargetId::canvas() {
                    (self.width, self.height)
                } else {
                    let l = self.layer(&p.space)?;
                    (l.raster.width(), l.raster.height())
                };
                if p.regions.iter().any(|r| {
                    r.x < 0.0
                        || r.y < 0.0
                        || r.x + r.width > f64::from(w)
                        || r.y + r.height > f64::from(h)
                }) {
                    return Err(PicError::new(
                        ErrorCode::InvalidArgument,
                        "selection rectangles must lie inside their coordinate space",
                    ));
                }
                self.selection = Some(p.clone());
            }
            LayerOperation::SelectionClear => self.selection = None,
            _ => {
                let i = self.index(target)?;
                match op {
                    LayerOperation::Parent(p) => {
                        self.layers[i].parent = p.parent.clone();
                        self.reorder(target, p.before.as_ref())?;
                    }
                    LayerOperation::Clip(p) => self.layers[i].clip = p.base.clone(),
                    LayerOperation::TextSet(p) => {
                        if !matches!(self.layers[i].kind, LayerKind::Text { .. }) {
                            return Err(PicError::new(
                                ErrorCode::InvalidTarget,
                                "text_set requires a text layer",
                            ));
                        }
                        let raster = crate::text::render(p, &load_font(&p.font)?, limits)?;
                        if let Some(mask) = &self.layers[i].mask {
                            Self::check_mask(&raster, mask)?;
                        }
                        self.layers[i].raster = raster;
                        self.layers[i].kind = LayerKind::Text { params: p.clone() };
                    }
                    LayerOperation::AdjustmentSet(p) => {
                        if !matches!(self.layers[i].kind, LayerKind::Adjustment { .. }) {
                            return Err(PicError::new(
                                ErrorCode::InvalidTarget,
                                "adjustment_set requires an adjustment layer",
                            ));
                        }
                        self.layers[i].kind = LayerKind::Adjustment { params: p.clone() };
                    }
                    LayerOperation::Set(p) => {
                        let l = &mut self.layers[i];
                        if let Some(v) = &p.name {
                            l.name = v.clone();
                        }
                        if let Some(v) = p.visible {
                            l.visible = v;
                        }
                        if let Some(v) = p.opacity {
                            l.opacity = v;
                        }
                        if let Some(v) = p.blend {
                            l.blend = v;
                        }
                    }
                    LayerOperation::Transform(p) => self.layers[i].transform = p.clone(),
                    LayerOperation::MaskSet(p) => {
                        let mask = load(&p.source, true)?;
                        Self::check_mask(&self.layers[i].raster, &mask)?;
                        self.layers[i].mask = Some(mask);
                    }
                    LayerOperation::MaskRemove => self.layers[i].mask = None,
                    LayerOperation::Remove => {
                        if self.layers.iter().any(|l| {
                            l.parent.as_ref() == Some(target) || l.clip.as_ref() == Some(target)
                        }) {
                            return Err(PicError::new(
                                ErrorCode::InvalidHierarchy,
                                "reparent children and detach clipping dependents before removing their target",
                            ));
                        }
                        self.layers.remove(i);
                        if self.selection.as_ref().is_some_and(|s| &s.space == target) {
                            self.selection = None;
                        }
                    }
                    LayerOperation::Reorder(p) => {
                        self.reorder(target, p.before.as_ref())?;
                    }
                    _ => unreachable!(),
                }
            }
        }
        self.layered = true;
        Ok(())
    }

    fn insert_layer(&mut self, layer: Layer) -> Result<()> {
        if !self.used_layer_ids.insert(layer.id.0.clone()) {
            return Err(PicError::new(
                ErrorCode::InvalidTarget,
                "layer ID has already been used in this history",
            ));
        }
        self.layers.push(layer);
        Ok(())
    }

    fn reorder(&mut self, target: &TargetId, before: Option<&TargetId>) -> Result<()> {
        let index = self.index(target)?;
        if before == Some(target) {
            return Err(PicError::new(
                ErrorCode::InvalidTarget,
                "cannot reorder a layer relative to itself",
            ));
        }
        if let Some(before) = before
            && self.layer(before)?.parent != self.layers[index].parent
        {
            return Err(PicError::new(
                ErrorCode::InvalidHierarchy,
                "before must name a sibling in the destination group",
            ));
        }
        let layer = self.layers.remove(index);
        let next = before
            .map(|id| self.index(id))
            .transpose()?
            .unwrap_or(self.layers.len());
        self.layers.insert(next, layer);
        Ok(())
    }
}
