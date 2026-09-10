use super::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub enum UploadData<'a> {
    Image(ModelImage<'a>),
    Model {
        vertices: &'a [[f32; 8]],
        indices: &'a [u32],
        parts: Vec<ModelPart<'a>>,
    },
}
/// Implementations must retain the same immutable data for the upload lifetime.
/// This boundary lets the asset bridge own CPU snapshots without a renderer ->
/// importer dependency or an extra copy of decoded texture pixels.
pub trait UploadSource: Send + Sync {
    fn data(&self) -> UploadData<'_>;
}

#[derive(Clone, Copy, Debug)]
pub struct UploadProgress {
    pub bytes_done: usize,
    pub bytes_total: usize,
    /// Buffer/texture payload plus generated mip output bytes in this slice.
    pub slice_bytes: usize,
    pub complete: bool,
    pub prepare_ms: f64,
    pub cpu_ms: f64,
}
enum BufferSource {
    Vertices,
    Indices(Option<usize>),
    Shading(usize),
}
struct BufferWrite {
    buffer: wgpu::Buffer,
    source: BufferSource,
    offset: usize,
    count: usize,
    stride: usize,
}
struct ImageWrite {
    texture: wgpu::Texture,
    source: Option<(usize, usize)>,
    level: u32,
    row: u32,
    translucent: bool,
    srgb: bool,
}
enum Target {
    Image,
    Mesh(MeshBuffers),
    Model {
        parts: Vec<UploadedPart>,
        base_images: Vec<Option<usize>>,
    },
}
/// Staged GPU resources are invisible until finish. Dropping this value cancels
/// publication; queued GPU work can finish safely against its retained handles.
pub struct PendingUpload {
    source: Arc<dyn UploadSource>,
    target: Target,
    buffers: Vec<BufferWrite>,
    images: Vec<ImageWrite>,
    buffer_cursor: usize,
    image_cursor: usize,
    done: usize,
    total: usize,
    minimum: usize,
    prepare_ms: f64,
    cpu_ms: f64,
    slices: usize,
    max_slice_bytes: usize,
    max_slice_cpu_ms: f64,
}

fn image_at<'a>(data: &'a UploadData<'a>, locator: Option<(usize, usize)>) -> ModelImage<'a> {
    match (data, locator) {
        (UploadData::Image(image), None) => image.clone(),
        (UploadData::Model { parts, .. }, Some((part, slot))) => {
            let part = &parts[part];
            if slot == 0 {
                return part.image.as_ref().unwrap().clone();
            }
            let s = part.shading.as_ref().unwrap();
            [&s.normal, &s.metallic_roughness, &s.occlusion, &s.emissive][slot - 1]
                .as_ref()
                .unwrap()
                .image
                .clone()
        }
        _ => unreachable!("validated immutable upload source"),
    }
}

#[derive(Clone)]
pub struct UploadContext {
    pbr: crate::pbr::PbrRenderer,
    model_sampler: wgpu::Sampler,
}
impl SceneRenderer {
    pub fn upload_context(&self) -> UploadContext {
        UploadContext {
            pbr: self.pbr.clone(),
            model_sampler: self.model_sampler.clone(),
        }
    }
    pub fn begin_upload(&self, gpu: &Gpu, source: Arc<dyn UploadSource>) -> Result<PendingUpload> {
        self.upload_context().begin_upload(gpu, source)
    }
}
impl UploadContext {
    pub fn begin_upload(&self, gpu: &Gpu, source: Arc<dyn UploadSource>) -> Result<PendingUpload> {
        let started = Instant::now();
        let data = source.data();
        let mut buffers = Vec::new();
        let mut images = Vec::new();
        let mut cache = BTreeMap::new();
        let mut buffer = |count: usize, stride: usize, usage, kind| -> Result<wgpu::Buffer> {
            let size = count
                .checked_mul(stride)
                .context("upload buffer size overflow")?;
            ensure!(
                size > 0 && size as u64 <= gpu.device.limits().max_buffer_size,
                "invalid upload buffer size"
            );
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staged asset buffer"),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            buffers.push(BufferWrite {
                buffer: buffer.clone(),
                source: kind,
                offset: 0,
                count,
                stride,
            });
            Ok(buffer)
        };
        let mut image = |pixels: ModelImage<'_>,
                         locator,
                         srgb,
                         mips: bool|
         -> Result<(usize, wgpu::TextureView)> {
            ensure!(
                pixels.width > 0
                    && pixels.height > 0
                    && pixels.width <= 4096
                    && pixels.height <= 4096
                    && pixels.width <= gpu.device.limits().max_texture_dimension_2d
                    && pixels.height <= gpu.device.limits().max_texture_dimension_2d
                    && pixels.rgba.len() == pixels.width as usize * pixels.height as usize * 4,
                "invalid staged image"
            );
            let key = (
                pixels.rgba.as_ptr() as usize,
                pixels.width,
                pixels.height,
                srgb,
            );
            if let Some((index, view)) = cache.get(&key) {
                return Ok((*index, wgpu::TextureView::clone(view)));
            }
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("staged asset texture"),
                size: wgpu::Extent3d {
                    width: pixels.width,
                    height: pixels.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: if mips {
                    crate::mipmap::levels(pixels.width, pixels.height)
                } else {
                    1
                },
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: if srgb {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let index = images.len();
            images.push(ImageWrite {
                texture,
                source: locator,
                level: 0,
                row: 0,
                translucent: false,
                srgb,
            });
            cache.insert(key, (index, view.clone()));
            Ok((index, view))
        };
        let target = match &data {
            UploadData::Image(pixels) => {
                image(pixels.clone(), None, true, false)?;
                Target::Image
            }
            UploadData::Model {
                vertices,
                indices,
                parts,
            } => {
                ensure!(
                    !vertices.is_empty()
                        && vertices.len() <= 1_000_000
                        && !indices.is_empty()
                        && indices.len() <= 3_000_000
                        && indices.len().is_multiple_of(3),
                    "invalid staged model size"
                );
                ensure!(
                    vertices.iter().flatten().all(|v| v.is_finite())
                        && indices.iter().all(|i| (*i as usize) < vertices.len()),
                    "invalid staged geometry"
                );
                ensure!(parts.len() <= 4096, "too many model surfaces");
                let shared = buffer(
                    vertices.len(),
                    32,
                    wgpu::BufferUsages::VERTEX,
                    BufferSource::Vertices,
                )?;
                if parts.is_empty() {
                    Target::Mesh(MeshBuffers {
                        vertices: shared,
                        indices: buffer(
                            indices.len(),
                            4,
                            wgpu::BufferUsages::INDEX,
                            BufferSource::Indices(None),
                        )?,
                        count: indices.len() as u32,
                        vertex_offset: 0,
                    })
                } else {
                    let mut uploaded = Vec::new();
                    let mut base_images = Vec::new();
                    for (part_index, part) in parts.iter().enumerate() {
                        let start = part.start as usize;
                        let end = start
                            .checked_add(part.count as usize)
                            .context("surface range overflow")?;
                        ensure!(
                            part.count > 0 && part.count.is_multiple_of(3) && end <= indices.len(),
                            "invalid staged surface range"
                        );
                        ensure!(
                            part.color
                                .iter()
                                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                                && part
                                    .alpha_cutoff
                                    .is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
                            "invalid staged material"
                        );
                        if let Some(s) = &part.shading {
                            s.validate()?;
                            let base = s.vertex_start as usize;
                            ensure!(
                                base.checked_add(s.vertices.len())
                                    .is_some_and(|end| end <= vertices.len())
                                    && indices[start..end].iter().all(|i| (*i as usize) >= base
                                        && (*i as usize) < base + s.vertices.len()),
                                "shading attributes do not cover surface"
                            );
                        }
                        let base = part
                            .image
                            .as_ref()
                            .map(|pixels| image(pixels.clone(), Some((part_index, 0)), true, true))
                            .transpose()?;
                        base_images.push(base.as_ref().map(|(i, _)| *i));
                        let shading = if let Some(s) = &part.shading {
                            let mut views = [None, None, None, None];
                            for (slot, map) in
                                [&s.normal, &s.metallic_roughness, &s.occlusion, &s.emissive]
                                    .into_iter()
                                    .enumerate()
                            {
                                if let Some(map) = map {
                                    views[slot] = Some(
                                        image(
                                            map.image.clone(),
                                            Some((part_index, slot + 1)),
                                            slot == 3,
                                            true,
                                        )?
                                        .1,
                                    );
                                }
                            }
                            Some(self.pbr.bind(
                                gpu,
                                s,
                                views,
                                buffer(
                                    s.vertices.len(),
                                    48,
                                    wgpu::BufferUsages::VERTEX,
                                    BufferSource::Shading(part_index),
                                )?,
                            ))
                        } else {
                            None
                        };
                        let mut min = Vec3::splat(f32::INFINITY);
                        let mut max = Vec3::splat(f32::NEG_INFINITY);
                        for i in &indices[start..end] {
                            let p = Vec3::from_slice(&vertices[*i as usize][..3]);
                            min = min.min(p);
                            max = max.max(p);
                        }
                        uploaded.push(UploadedPart {
                            mesh: MeshBuffers {
                                vertices: shared.clone(),
                                indices: buffer(
                                    part.count as usize,
                                    4,
                                    wgpu::BufferUsages::INDEX,
                                    BufferSource::Indices(Some(part_index)),
                                )?,
                                count: part.count,
                                vertex_offset: part
                                    .shading
                                    .as_ref()
                                    .map_or(0, |s| s.vertex_start as u64 * 32),
                            },
                            texture: base.map(|(_, view)| view),
                            color: part.color,
                            cutoff: part.alpha_cutoff,
                            translucent: part.color[3] < 1.,
                            center: min * 0.5 + max * 0.5,
                            sampler: part.shading.as_ref().map_or_else(
                                || self.model_sampler.clone(),
                                |s| gpu.device.create_sampler(&s.base_color_sampler),
                            ),
                            shading,
                        });
                    }
                    Target::Model {
                        parts: uploaded,
                        base_images,
                    }
                }
            }
        };
        let total = buffers.iter().map(|b| b.count * b.stride).sum::<usize>()
            + images
                .iter()
                .map(|i| {
                    (0..i.texture.mip_level_count())
                        .map(|level| {
                            ((i.texture.width() >> level).max(1)
                                * (i.texture.height() >> level).max(1)
                                * 4) as usize
                        })
                        .sum::<usize>()
                })
                .sum::<usize>();
        let minimum = buffers
            .iter()
            .map(|b| b.stride)
            .chain(images.iter().map(|i| i.texture.width() as usize * 4))
            .max()
            .unwrap_or(4);
        drop(data);
        Ok(PendingUpload {
            source,
            target,
            buffers,
            images,
            buffer_cursor: 0,
            image_cursor: 0,
            done: 0,
            total,
            minimum,
            prepare_ms: started.elapsed().as_secs_f64() * 1000.,
            cpu_ms: 0.,
            slices: 0,
            max_slice_bytes: 0,
            max_slice_cpu_ms: 0.,
        })
    }
}

impl PendingUpload {
    pub fn progress(&self) -> UploadProgress {
        UploadProgress {
            bytes_done: self.done,
            bytes_total: self.total,
            slice_bytes: 0,
            complete: self.done == self.total,
            prepare_ms: self.prepare_ms,
            cpu_ms: self.cpu_ms,
        }
    }
    pub fn advance(
        &mut self,
        gpu: &Gpu,
        renderer: &SceneRenderer,
        budget: usize,
    ) -> Result<UploadProgress> {
        ensure!(
            budget >= self.minimum,
            "upload budget must fit one row or vertex ({} bytes)",
            self.minimum
        );
        let started = Instant::now();
        let before = self.done;
        let data = self.source.data();
        while self.done - before < budget
            && (self.done == before || started.elapsed() < Duration::from_millis(4))
        {
            let remaining = budget - (self.done - before);
            if let Some(write) = self.buffers.get_mut(self.buffer_cursor) {
                let count =
                    (remaining.min(256 * 1024) / write.stride).min(write.count - write.offset);
                if count == 0 {
                    break;
                }
                let UploadData::Model {
                    vertices,
                    indices,
                    parts,
                } = &data
                else {
                    unreachable!()
                };
                let range = write.offset..write.offset + count;
                let bytes = match write.source {
                    BufferSource::Vertices => {
                        float_bytes(vertices[range].iter().flatten().copied())
                    }
                    BufferSource::Shading(part) => float_bytes(
                        parts[part].shading.as_ref().unwrap().vertices[range]
                            .iter()
                            .flatten()
                            .copied(),
                    ),
                    BufferSource::Indices(part) => {
                        let (start, base) = part.map_or((0, 0), |i| {
                            (
                                parts[i].start as usize,
                                parts[i].shading.as_ref().map_or(0, |s| s.vertex_start),
                            )
                        });
                        indices[start + range.start..start + range.end]
                            .iter()
                            .flat_map(|i| (i - base).to_le_bytes())
                            .collect()
                    }
                };
                gpu.queue
                    .write_buffer(&write.buffer, (write.offset * write.stride) as u64, &bytes);
                write.offset += count;
                self.done += count * write.stride;
                if write.offset == write.count {
                    self.buffer_cursor += 1;
                }
            } else if let Some(write) = self.images.get_mut(self.image_cursor) {
                let width = (write.texture.width() >> write.level).max(1);
                let height = (write.texture.height() >> write.level).max(1);
                let chunk = if write.level == 0 {
                    remaining.min(256 * 1024)
                } else {
                    remaining
                };
                let rows = (chunk / (width as usize * 4)).min((height - write.row) as usize) as u32;
                if rows == 0 {
                    break;
                }
                if write.level == 0 {
                    let pixels = image_at(&data, write.source);
                    let start = (write.row * width * 4) as usize;
                    let end = start + (rows * width * 4) as usize;
                    let bytes = &pixels.rgba[start..end];
                    write.translucent |= bytes.chunks_exact(4).any(|p| p[3] < 255);
                    gpu.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &write.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: 0,
                                y: write.row,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        bytes,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(width * 4),
                            rows_per_image: Some(rows),
                        },
                        wgpu::Extent3d {
                            width,
                            height: rows,
                            depth_or_array_layers: 1,
                        },
                    );
                } else {
                    let generator = if write.srgb {
                        &renderer.mipmaps
                    } else {
                        &renderer.linear_mipmaps
                    };
                    generator.generate_rows(gpu, &write.texture, write.level, write.row, rows);
                }
                write.row += rows;
                self.done += (width * rows * 4) as usize;
                if write.row == height {
                    write.row = 0;
                    write.level += 1;
                    if write.level == write.texture.mip_level_count() {
                        self.image_cursor += 1;
                    }
                }
            } else {
                break;
            }
        }
        // Initial loads may show a progress panel instead of rendering a scene.
        // Flush writes anyway so queue staging memory cannot accumulate across frames.
        if self.done != before {
            gpu.queue.submit([]);
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
        self.cpu_ms += elapsed_ms;
        self.max_slice_cpu_ms = self.max_slice_cpu_ms.max(elapsed_ms);
        let mut progress = self.progress();
        progress.slice_bytes = self.done - before;
        self.slices += 1;
        self.max_slice_bytes = self.max_slice_bytes.max(progress.slice_bytes);
        Ok(progress)
    }
    pub fn finish(self, renderer: &mut SceneRenderer, id: &str) -> Result<()> {
        ensure!(
            self.done == self.total,
            "cannot publish an incomplete GPU upload"
        );
        renderer.remove_asset(id);
        match self.target {
            Target::Image => {
                renderer.imported_textures.insert(
                    id.into(),
                    self.images[0].texture.create_view(&Default::default()),
                );
                if self.images[0].translucent {
                    renderer.transparent_textures.insert(id.into());
                }
            }
            Target::Mesh(mesh) => {
                renderer.imported_meshes.insert(id.into(), mesh);
            }
            Target::Model {
                mut parts,
                base_images,
            } => {
                for (part, image) in parts.iter_mut().zip(base_images) {
                    if let Some(i) = image {
                        part.translucent |= self.images[i].translucent;
                    }
                }
                renderer.model_upload_stats.insert(
                    id.into(),
                    ModelUploadStats {
                        surfaces: parts.len(),
                        unique_images: self.images.len(),
                        texture_bytes: self
                            .images
                            .iter()
                            .map(|i| {
                                crate::mipmap::texture_bytes(i.texture.width(), i.texture.height())
                            })
                            .sum(),
                        cpu_upload_ms: self.prepare_ms + self.cpu_ms,
                        prepare_ms: self.prepare_ms,
                        upload_slices: self.slices,
                        max_slice_bytes: self.max_slice_bytes,
                        max_slice_cpu_ms: self.max_slice_cpu_ms,
                    },
                );
                renderer.models.insert(id.into(), parts);
            }
        }
        Ok(())
    }
}
