use serde::Serialize;

use crate::{
    ErrorCode, PicError, codec,
    document::{RevisionId, TargetId},
    operation::{
        Operation,
        geometry::{CropParams, Interpolation, ResizeParams},
    },
    pipeline::{PIPELINE_SCHEMA_VERSION, Pipeline, PipelineSpec},
    result::{Diagnostics, Warning, timed},
};

use super::{
    CacheHit, ExportRequest, ImageSize, Project, ProjectExport, ReplayReport, Result,
    cache::{CachedRaster, json_key},
    storage::DerivedKind,
};

pub struct PreviewRequest<'a> {
    pub export: ExportRequest<'a>,
    pub target: TargetId,
    pub region: Option<CropParams>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub filter: Interpolation,
}

#[derive(Debug, Serialize)]
pub struct AffineMapping {
    pub scale: [f64; 2],
    pub offset: [f64; 2],
}

impl AffineMapping {
    pub fn map(&self, point: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| point[i] * self.scale[i] + self.offset[i])
    }
}

#[derive(Debug, Serialize)]
pub struct CoordinateMapping {
    /// Continuous pixel-edge coordinates: pixel index (i,j) has center (i+0.5,j+0.5).
    pub convention: &'static str,
    pub preview_to_canvas: AffineMapping,
    pub canvas_to_preview: AffineMapping,
    pub layer_to_canvas: Option<crate::composite::Affine>,
    pub canvas_to_layer: Option<crate::composite::Affine>,
    pub preview_to_layer: Option<crate::composite::Affine>,
    pub layer_to_preview: Option<crate::composite::Affine>,
}

impl CoordinateMapping {
    fn new(region: &CropParams, size: ImageSize, layer: Option<crate::composite::Affine>) -> Self {
        let scale = [
            f64::from(region.width) / f64::from(size.width),
            f64::from(region.height) / f64::from(size.height),
        ];
        let offset = [f64::from(region.x), f64::from(region.y)];
        let preview =
            crate::composite::Affine([scale[0], 0.0, 0.0, scale[1], offset[0], offset[1]]);
        Self {
            layer_to_canvas: layer,
            canvas_to_layer: layer.map(|m| m.inverse()),
            preview_to_layer: layer.map(|m| m.inverse().then(preview)),
            layer_to_preview: layer.map(|m| preview.inverse().then(m)),
            convention: "pixel_edges; centers=(index+0.5); x_right_y_down",
            preview_to_canvas: AffineMapping { scale, offset },
            canvas_to_preview: AffineMapping {
                scale: scale.map(|s| 1.0 / s),
                offset: std::array::from_fn(|i| -offset[i] / scale[i]),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProjectPreview {
    #[serde(flatten)]
    pub rendered: ProjectExport,
    pub canvas: ImageSize,
    pub region: CropParams,
    pub preview_size: ImageSize,
    pub filter: Interpolation,
    pub coordinates: CoordinateMapping,
}

impl PreviewRequest<'_> {
    fn geometry(&self, canvas: ImageSize) -> Result<(CropParams, ImageSize)> {
        let region = self.region.clone().unwrap_or(CropParams {
            x: 0,
            y: 0,
            width: canvas.width,
            height: canvas.height,
        });
        region.validate()?;
        if region.x + region.width > canvas.width || region.y + region.height > canvas.height {
            return Err(PicError::new(
                ErrorCode::InvalidArgument,
                "preview region must lie inside the selected revision's canvas",
            ));
        }
        let size = if self.width.is_none() && self.height.is_none() {
            ImageSize {
                width: region.width,
                height: region.height,
            }
        } else {
            let (width, height) = ResizeParams {
                width: self.width,
                height: self.height,
                filter: self.filter,
            }
            .dimensions(region.width, region.height)?;
            ImageSize { width, height }
        };
        Ok((region, size))
    }
}

impl Project {
    pub fn preview(
        &self,
        request: PreviewRequest<'_>,
        diagnostics: &mut Diagnostics,
    ) -> Result<ProjectPreview> {
        let (output, encoding) = timed(&mut diagnostics.timings.validation_ms, || {
            if let Some(region) = &request.region {
                region.validate()?;
            }
            let encoding = request.export.encoding.resolve(request.export.output)?;
            let output = codec::prepare_output(request.export.output, request.export.overwrite)?;
            if output.starts_with(self.path()) {
                return Err(PicError::new(
                    ErrorCode::UnsafePath,
                    "preview output must be outside the project directory",
                ));
            }
            Ok((output, encoding))
        })?;
        let revision = request
            .export
            .revision
            .unwrap_or(&self.manifest.current_revision.0);
        let steps = self.history.path(revision)?;
        // Validate the authority before either cache can supply pixels.
        self.verified_source(&steps, diagnostics)?;
        let keys = self.prefix_keys(&steps)?;
        let key = json_key(&serde_json::json!({
            "preview_schema": 1, "prefix": keys.last(), "target": request.target,
            "region": request.region, "width": request.width, "height": request.height,
            "filter": request.filter,
        }))?;
        let hit = self
            .cache_get(DerivedKind::Preview, &key, diagnostics)
            .filter(|(value, _)| {
                value.document.is_none()
                    && value.layer_to_canvas.is_some() == (request.target != TargetId::canvas())
                    && request
                        .geometry(value.canvas)
                        .is_ok_and(|(_, size)| size == ImageSize::of(&value.raster))
            });
        let (value, replay) = if let Some((value, tier)) = hit {
            (
                value,
                ReplayReport {
                    cache_hit: Some(CacheHit {
                        kind: "preview",
                        tier,
                        key: key.clone(),
                        revision: RevisionId(revision.into()),
                    }),
                    reused_steps: steps.len(),
                    recomputed_revisions: Vec::new(),
                },
            )
        } else {
            let restored = self.restore(Some(revision), diagnostics)?;
            let canvas = ImageSize::of(&restored.raster);
            let layer_to_canvas = restored.document.target_mapping(&request.target)?;
            let target_raster = if request.target == TargetId::canvas() {
                restored.raster
            } else {
                timed(&mut diagnostics.timings.process_ms, || {
                    restored
                        .document
                        .render_for_preview(&request.target, &self.limits)
                })?
            };
            let (region, size) = request.geometry(canvas)?;
            self.limits.check_dimensions(size.width, size.height)?;
            let raster = timed(&mut diagnostics.timings.process_ms, || {
                let mut operations = Vec::new();
                if region.x != 0
                    || region.y != 0
                    || region.width != canvas.width
                    || region.height != canvas.height
                {
                    operations.push(Operation::Crop(region).to_spec());
                }
                if size != ImageSize::of(&target_raster) || !operations.is_empty() {
                    operations.push(
                        Operation::Resize(ResizeParams {
                            width: Some(size.width),
                            height: Some(size.height),
                            filter: request.filter,
                        })
                        .to_spec(),
                    );
                }
                // Same execution core as edit/export; temporary observation ops never enter history.
                let raster = Pipeline::new(
                    PipelineSpec {
                        schema_version: PIPELINE_SCHEMA_VERSION,
                        operations,
                    },
                    self.path(),
                    &self.limits,
                )?
                .execute(target_raster)?
                .raster;
                if request.target.0.starts_with("mask:") {
                    crate::composite::visualize_coverage(&raster, &self.limits)
                } else {
                    Ok(raster)
                }
            })?;
            let value = CachedRaster {
                raster,
                canvas,
                document: None,
                layer_to_canvas,
            };
            self.cache_put(DerivedKind::Preview, &key, value.clone(), diagnostics);
            (value, restored.replay)
        };
        let (region, size) = request.geometry(value.canvas)?;
        let bytes = timed(&mut diagnostics.timings.encode_ms, || {
            codec::encode(&value.raster, &encoding, &self.limits)
        })?;
        timed(&mut diagnostics.timings.write_ms, || {
            codec::publish(&output, &bytes, request.export.overwrite)
        })?;
        diagnostics.warnings.push(Warning {
            code: "metadata_not_preserved",
            message: "Export writes pixels only; source metadata is not preserved.",
        });
        if value.raster.has_transparency() && encoding.jpeg_background.is_some() {
            diagnostics.warnings.push(Warning { code: "alpha_flattened", message: "JPEG export composites transparency over the explicit background in linear sRGB." });
        }
        Ok(ProjectPreview {
            rendered: ProjectExport {
                project: self.path().to_owned(),
                revision: RevisionId(revision.into()),
                target: request.target,
                op_id: self
                    .history
                    .step(revision)
                    .map(|(_, step)| step.op_id.clone()),
                output,
                width: size.width,
                height: size.height,
                operations_replayed: replay.recomputed_revisions.len(),
                replay,
                format: encoding.format,
                jpeg_quality: encoding.jpeg_quality,
                png_compression: encoding.png_compression,
                jpeg_background: encoding.jpeg_background,
            },
            canvas: value.canvas,
            coordinates: CoordinateMapping::new(&region, size, value.layer_to_canvas),
            region,
            preview_size: size,
            filter: request.filter,
        })
    }
}
