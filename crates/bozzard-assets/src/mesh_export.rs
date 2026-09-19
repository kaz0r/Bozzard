//! Portable output for generated static geometry; the normal glTF importer validates it again.
use crate::{Filter, ImageData, MeshData, MeshPart, Sampler, Wrap, job::Progress};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::ImageEncoder;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}
impl Buffer {
    fn accessor(
        &mut self,
        bytes: impl IntoIterator<Item = u8>,
        count: usize,
        component: u32,
        kind: &str,
    ) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = self.bytes.len();
        self.bytes.extend(bytes);
        self.views
            .push(json!({"buffer":0,"byteOffset":offset,"byteLength":self.bytes.len()-offset}));
        let index = self.accessors.len();
        self.accessors.push(json!({"bufferView":self.views.len()-1,"componentType":component,"count":count,"type":kind}));
        index
    }
}
#[derive(Default)]
struct Images {
    images: Vec<Value>,
    textures: Vec<Value>,
    samplers: Vec<Value>,
    // Identity is safe while the immutable source mesh retains every Arc.
    shared: HashMap<usize, usize>,
}
impl Images {
    fn texture(&mut self, image: &Arc<ImageData>, sampler: Sampler) -> Result<usize> {
        let key = Arc::as_ptr(image) as usize;
        let source = if let Some(index) = self.shared.get(&key) {
            *index
        } else {
            ensure!(
                (1..=8192).contains(&image.width)
                    && (1..=8192).contains(&image.height)
                    && image.rgba.len() as u64
                        == u64::from(image.width) * u64::from(image.height) * 4,
                "invalid generated texture dimensions"
            );
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png).write_image(
                &image.rgba,
                image.width,
                image.height,
                image::ExtendedColorType::Rgba8,
            )?;
            let index = self.images.len();
            self.images
                .push(json!({"uri":format!("data:image/png;base64,{}",STANDARD.encode(png))}));
            self.shared.insert(key, index);
            index
        };
        let wrap = |w| match w {
            Wrap::Repeat => 10497,
            Wrap::Clamp => 33071,
            Wrap::Mirror => 33648,
        };
        let filter = |f| match f {
            Filter::Nearest => 9728,
            Filter::Linear => 9729,
        };
        let min = match (sampler.min, sampler.mip) {
            (f, None) => filter(f),
            (Filter::Nearest, Some(Filter::Nearest)) => 9984,
            (Filter::Linear, Some(Filter::Nearest)) => 9985,
            (Filter::Nearest, Some(Filter::Linear)) => 9986,
            (Filter::Linear, Some(Filter::Linear)) => 9987,
        };
        let value = json!({"magFilter":filter(sampler.mag),"minFilter":min,"wrapS":wrap(sampler.wrap_u),"wrapT":wrap(sampler.wrap_v)});
        let sampler = self
            .samplers
            .iter()
            .position(|s| *s == value)
            .unwrap_or_else(|| {
                self.samplers.push(value);
                self.samplers.len() - 1
            });
        let texture = json!({"source":source,"sampler":sampler});
        Ok(self
            .textures
            .iter()
            .position(|t| *t == texture)
            .unwrap_or_else(|| {
                self.textures.push(texture);
                self.textures.len() - 1
            }))
    }
}

/// Serialize a complete static mesh to self-contained glTF, including all supported
/// PBR attributes, samplers, alpha modes and shared image data. No external files.
pub fn mesh_gltf(mesh: &MeshData, progress: &Progress) -> Result<Vec<u8>> {
    crate::simplify::validate_mesh(mesh)?;
    let mut buffer = Buffer::default();
    let mut images = Images::default();
    let mut materials = Vec::new();
    let mut primitives = Vec::new();
    let fallback = MeshPart {
        source_key: String::new(),
        name: "Mesh".into(),
        material_name: None,
        start: 0,
        count: mesh.indices.len() as u32,
        color: [1.; 4],
        image: None,
        alpha_cutoff: None,
        shading: None,
    };
    let parts = if mesh.parts.is_empty() {
        std::slice::from_ref(&fallback)
    } else {
        &mesh.parts
    };
    for (part_index, part) in parts.iter().enumerate() {
        progress.stage(format!(
            "Encoding LOD surface {}/{}",
            part_index + 1,
            parts.len()
        ))?;
        let mut remap = BTreeMap::new();
        let mut vertices = Vec::new();
        let indices: Vec<u32> = mesh.indices
            [part.start as usize..(part.start + part.count) as usize]
            .iter()
            .map(|&i| {
                *remap.entry(i).or_insert_with(|| {
                    let next = vertices.len() as u32;
                    vertices.push(i as usize);
                    next
                })
            })
            .collect();
        let mut attributes = serde_json::Map::new();
        for (name, range, kind) in [
            ("POSITION", 0..3, "VEC3"),
            ("NORMAL", 3..6, "VEC3"),
            ("TEXCOORD_0", 6..8, "VEC2"),
        ] {
            let index = buffer.accessor(
                vertices.iter().flat_map(|&v| {
                    mesh.vertices[v][range.clone()]
                        .iter()
                        .flat_map(|x| x.to_le_bytes())
                }),
                vertices.len(),
                5126,
                kind,
            );
            if name == "POSITION" {
                let mut min = [f32::INFINITY; 3];
                let mut max = [f32::NEG_INFINITY; 3];
                for &i in &vertices {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(mesh.vertices[i][axis]);
                        max[axis] = max[axis].max(mesh.vertices[i][axis]);
                    }
                }
                buffer.accessors[index]["min"] = json!(min);
                buffer.accessors[index]["max"] = json!(max);
            }
            attributes.insert(name.into(), json!(index));
        }
        let mut material = json!({"pbrMetallicRoughness":{"baseColorFactor":part.color}});
        if let Some(s) = &part.shading {
            for (name, range, kind) in [
                ("TANGENT", 0..4, "VEC4"),
                ("TEXCOORD_1", 4..6, "VEC2"),
                ("TEXCOORD_2", 6..8, "VEC2"),
                ("TEXCOORD_3", 8..10, "VEC2"),
                ("TEXCOORD_4", 10..12, "VEC2"),
            ] {
                let index = buffer.accessor(
                    vertices.iter().flat_map(|&v| {
                        s.vertices[v - s.vertex_start as usize][range.clone()]
                            .iter()
                            .flat_map(|x| x.to_le_bytes())
                    }),
                    vertices.len(),
                    5126,
                    kind,
                );
                attributes.insert(name.into(), json!(index));
            }
            let m = &s.material;
            material["pbrMetallicRoughness"]["metallicFactor"] = json!(m.metallic);
            material["pbrMetallicRoughness"]["roughnessFactor"] = json!(m.roughness);
            material["emissiveFactor"] = json!(m.emissive_factor);
            material["doubleSided"] = json!(m.double_sided);
            if let Some(map) = &m.normal {
                material["normalTexture"] = json!({"index":images.texture(&map.image,map.sampler)?,"texCoord":1,"scale":m.normal_scale});
            }
            if let Some(map) = &m.metallic_roughness {
                material["pbrMetallicRoughness"]["metallicRoughnessTexture"] =
                    json!({"index":images.texture(&map.image,map.sampler)?,"texCoord":2});
            }
            if let Some(map) = &m.occlusion {
                material["occlusionTexture"] = json!({"index":images.texture(&map.image,map.sampler)?,"texCoord":3,"strength":m.occlusion_strength});
            }
            if let Some(map) = &m.emissive {
                material["emissiveTexture"] =
                    json!({"index":images.texture(&map.image,map.sampler)?,"texCoord":4});
            }
        }
        if let Some(image) = &part.image {
            let sampler = part
                .shading
                .as_ref()
                .map_or_else(Sampler::default, |s| s.material.base_color_sampler);
            material["pbrMetallicRoughness"]["baseColorTexture"] =
                json!({"index":images.texture(image,sampler)?});
        }
        if let Some(name) = &part.material_name {
            material["name"] = json!(name);
        }
        if let Some(cutoff) = part.alpha_cutoff {
            material["alphaMode"] = json!("MASK");
            material["alphaCutoff"] = json!(cutoff);
        } else if part.color[3] < 1.
            || part
                .image
                .as_ref()
                .is_some_and(|i| i.rgba.chunks_exact(4).any(|p| p[3] != 255))
        {
            material["alphaMode"] = json!("BLEND");
        }
        let index = buffer.accessor(
            indices.iter().flat_map(|i| i.to_le_bytes()),
            indices.len(),
            5125,
            "SCALAR",
        );
        primitives.push(
            json!({"attributes":attributes,"indices":index,"material":materials.len(),"mode":4}),
        );
        materials.push(material);
    }
    progress.check()?;
    let result=serde_json::to_vec(&json!({"asset":{"version":"2.0","generator":"Bozzard mesh pipeline"},"buffers":[{"byteLength":buffer.bytes.len(),"uri":format!("data:application/octet-stream;base64,{}",STANDARD.encode(buffer.bytes))}],"bufferViews":buffer.views,"accessors":buffer.accessors,"images":images.images,"textures":images.textures,"samplers":images.samplers,"materials":materials,"meshes":[{"primitives":primitives}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0})).context("encoding generated mesh")?;
    ensure!(
        result.len() as u64 <= crate::MAX_SOURCE_BYTES,
        "generated mesh exceeds the 32 MiB import limit"
    );
    Ok(result)
}
