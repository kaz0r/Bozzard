//! glTF skin and clip cooking. Flattened mesh vertices retain a binding back to their source node.
use anyhow::{Context, Result, ensure};
use bozzard_scene::middleware::{
    animation::data::{
        Binding, Channel, Clip, Joint, MAX_BINDINGS, MAX_NODES, Pose, Property, Rig,
    },
    curve::{Curve, Interpolation, Key},
};
use glam::Mat4;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Skin {
    pub rig: Arc<Rig>,
    pub vertices: Vec<[u32; 8]>,
}
pub(super) struct Import {
    pub rig: Rig,
    pub vertices: Vec<[u32; 8]>,
    mapping: Vec<usize>,
    bindings: Vec<Option<Vec<u32>>>,
}
impl Import {
    pub fn new(gltf: &gltf::Gltf, buffers: &[Vec<u8>]) -> Result<Option<Self>> {
        if gltf.skins().next().is_none() && gltf.animations().next().is_none() {
            return Ok(None);
        }
        let nodes: Vec<_> = gltf.nodes().collect();
        ensure!(
            nodes.len() <= MAX_NODES,
            "animated model exceeds {MAX_NODES} nodes"
        );
        let mut parents = vec![None; nodes.len()];
        for node in &nodes {
            for child in node.children() {
                ensure!(
                    parents[child.index()].replace(node.index()).is_none(),
                    "animated node has multiple parents"
                );
            }
        }
        let mut order = Vec::with_capacity(nodes.len());
        let mut marks = vec![0; nodes.len()];
        fn visit(
            index: usize,
            parents: &[Option<usize>],
            marks: &mut [u8],
            order: &mut Vec<usize>,
            depth: usize,
        ) -> Result<()> {
            ensure!(
                depth <= 256 && marks[index] != 1,
                "cyclic or too deep animation hierarchy"
            );
            if marks[index] == 2 {
                return Ok(());
            }
            marks[index] = 1;
            if let Some(parent) = parents[index] {
                visit(parent, parents, marks, order, depth + 1)?;
            }
            marks[index] = 2;
            order.push(index);
            Ok(())
        }
        for index in 0..nodes.len() {
            visit(index, &parents, &mut marks, &mut order, 0)?;
        }
        let mut mapping = vec![0; nodes.len()];
        for (new, &old) in order.iter().enumerate() {
            mapping[old] = new;
        }
        let mut rig = Rig::default();
        for &old in &order {
            let node = &nodes[old];
            let (translation, rotation, scale) = node.transform().decomposed();
            let rest = Pose {
                translation,
                rotation,
                scale,
            };
            rest.validate()?;
            let matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
            ensure!(
                rest.matrix().abs_diff_eq(matrix, 0.001),
                "animated node matrix cannot contain shear"
            );
            rig.nodes.push(Joint {
                name: super::inspection_name(node.name().unwrap_or("Joint")),
                parent: parents[old].map(|p| mapping[p] as u32),
                rest,
            });
        }
        let mut names = std::collections::BTreeSet::new();
        for animation in gltf.animations() {
            let mut name = super::inspection_name(animation.name().unwrap_or("Animation"));
            if !names.insert(name.clone()) {
                name = format!("{name} {}", animation.index());
                ensure!(names.insert(name.clone()), "duplicate animation name");
            }
            let mut clip = Clip {
                name,
                duration: 0.001,
                channels: Vec::new(),
                events: Vec::new(),
            };
            for channel in animation.channels() {
                let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let times: Vec<_> = reader
                    .read_inputs()
                    .context("animation times missing")?
                    .collect();
                ensure!(
                    !times.is_empty() && times.len() <= bozzard_scene::middleware::curve::MAX_KEYS,
                    "animation channel exceeds key limit"
                );
                clip.duration = clip.duration.max(*times.last().unwrap());
                let mode = match channel.sampler().interpolation() {
                    gltf::animation::Interpolation::Step => Interpolation::Step,
                    gltf::animation::Interpolation::Linear => Interpolation::Linear,
                    gltf::animation::Interpolation::CubicSpline => Interpolation::Cubic,
                };
                use gltf::animation::util::ReadOutputs;
                let (property, values): (Property, Vec<Vec<f32>>) =
                    match reader.read_outputs().context("animation values missing")? {
                        ReadOutputs::Translations(values) => {
                            (Property::Translation, values.map(|v| v.to_vec()).collect())
                        }
                        ReadOutputs::Rotations(values) => (
                            Property::Rotation,
                            values.into_f32().map(|v| v.to_vec()).collect(),
                        ),
                        ReadOutputs::Scales(values) => {
                            (Property::Scale, values.map(|v| v.to_vec()).collect())
                        }
                        ReadOutputs::MorphTargetWeights(_) => anyhow::bail!(
                            "morph animation is not supported; export skeletal animation"
                        ),
                    };
                let stride = if mode == Interpolation::Cubic { 3 } else { 1 };
                ensure!(
                    values.len() == times.len() * stride,
                    "animation input/output sample count mismatch"
                );
                let axes = if property == Property::Rotation { 4 } else { 3 };
                let curves = (0..axes)
                    .map(|axis| Curve {
                        interpolation: mode,
                        keys: times
                            .iter()
                            .enumerate()
                            .map(|(index, &time)| {
                                if stride == 3 {
                                    Key {
                                        time,
                                        incoming: values[index * 3][axis],
                                        value: values[index * 3 + 1][axis],
                                        outgoing: values[index * 3 + 2][axis],
                                    }
                                } else {
                                    Key::new(time, values[index][axis])
                                }
                            })
                            .collect(),
                    })
                    .collect();
                clip.channels.push(Channel {
                    node: mapping[channel.target().node().index()] as u32,
                    property,
                    curves,
                });
            }
            rig.clips.push(clip);
        }
        rig.validate()?;
        Ok(Some(Self {
            rig,
            vertices: Vec::new(),
            mapping,
            bindings: vec![None; nodes.len()],
        }))
    }
    pub fn primitive(
        &mut self,
        node: &gltf::Node<'_>,
        primitive: &gltf::Primitive<'_>,
        transform: Mat4,
        buffers: &[Vec<u8>],
        count: usize,
    ) -> Result<()> {
        if self.bindings[node.index()].is_none() {
            let mut indices = Vec::new();
            if let Some(skin) = node.skin() {
                let joints: Vec<_> = skin.joints().collect();
                let inverse: Vec<_> = skin
                    .reader(|b| buffers.get(b.index()).map(Vec::as_slice))
                    .read_inverse_bind_matrices()
                    .map(|v| v.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                    .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
                ensure!(
                    !joints.is_empty() && inverse.len() == joints.len(),
                    "skin inverse bind count does not match joints"
                );
                for (joint, inverse) in joints.into_iter().zip(inverse) {
                    indices.push(self.rig.bindings.len() as u32);
                    self.rig.bindings.push(Binding {
                        node: self.mapping[joint.index()] as u32,
                        inverse_bind: (inverse * transform.inverse()).to_cols_array(),
                    });
                }
            } else {
                indices.push(self.rig.bindings.len() as u32);
                self.rig.bindings.push(Binding {
                    node: self.mapping[node.index()] as u32,
                    inverse_bind: transform.inverse().to_cols_array(),
                });
            }
            ensure!(
                self.rig.bindings.len() <= MAX_BINDINGS,
                "model exceeds {MAX_BINDINGS} skin bindings"
            );
            self.bindings[node.index()] = Some(indices);
        }
        let bindings = self.bindings[node.index()].as_ref().unwrap();
        if node.skin().is_some() {
            let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            ensure!(
                reader.read_joints(1).is_none() && reader.read_weights(1).is_none(),
                "skin supports four influences per vertex; reduce weights during export"
            );
            let joints: Vec<_> = reader
                .read_joints(0)
                .context("skinned primitive lacks JOINTS_0")?
                .into_u16()
                .collect();
            let weights: Vec<_> = reader
                .read_weights(0)
                .context("skinned primitive lacks WEIGHTS_0")?
                .into_f32()
                .collect();
            ensure!(
                joints.len() == count && weights.len() == count,
                "skin attribute count does not match positions"
            );
            for (joints, mut weights) in joints.into_iter().zip(weights) {
                ensure!(
                    joints.iter().all(|&j| (j as usize) < bindings.len()),
                    "skin joint index out of range"
                );
                ensure!(
                    weights.iter().all(|w| w.is_finite() && *w >= 0.),
                    "invalid skin weight"
                );
                let sum: f32 = weights.iter().sum();
                ensure!(sum.is_finite() && sum > 0., "skin weights sum to zero");
                for weight in &mut weights {
                    *weight /= sum;
                }
                self.vertices.push(std::array::from_fn(|i| {
                    if i < 4 {
                        bindings[joints[i] as usize]
                    } else {
                        weights[i - 4].to_bits()
                    }
                }));
            }
        } else {
            self.vertices.extend((0..count).map(|_| {
                [
                    bindings[0],
                    bindings[0],
                    bindings[0],
                    bindings[0],
                    1f32.to_bits(),
                    0,
                    0,
                    0,
                ]
            }));
        }
        Ok(())
    }
    pub fn finish(self, count: usize) -> Result<Skin> {
        ensure!(
            self.vertices.len() == count,
            "skin vertex mapping is incomplete"
        );
        self.rig.validate()?;
        Ok(Skin {
            rig: Arc::new(self.rig),
            vertices: self.vertices,
        })
    }
}
