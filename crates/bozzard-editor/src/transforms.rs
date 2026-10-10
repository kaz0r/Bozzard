//! Authored world matrices, composed once per document revision.
use super::*;
use std::{collections::HashMap, sync::Arc};

/// World matrices in document order, equal to `Scene::global_transforms`. Documents are
/// validated when published, so composing them does not run the validator again.
pub struct WorldTransforms {
    parents: Vec<Option<usize>>,
    matrices: Vec<Mat4>,
}
impl WorldTransforms {
    pub(crate) fn compose(scene: &Scene) -> Result<Self> {
        let indices: HashMap<&str, usize> = scene
            .objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.id.as_str(), index))
            .collect();
        let parents = scene
            .objects
            .iter()
            .map(|object| {
                object
                    .parent
                    .as_deref()
                    .map(|parent| {
                        indices.get(parent).copied().with_context(|| {
                            format!("missing parent '{parent}' on '{}'", object.id)
                        })
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let mut composed: Vec<Option<Mat4>> = vec![None; parents.len()];
        let mut chain = Vec::new();
        for start in 0..parents.len() {
            let mut index = start;
            while composed[index].is_none() {
                ensure!(
                    chain.len() < parents.len(),
                    "scene transform hierarchy contains a cycle"
                );
                chain.push(index);
                match parents[index] {
                    Some(parent) => index = parent,
                    None => break,
                }
            }
            // Same arithmetic as the validator's breadth-first pass: parent, then local.
            while let Some(index) = chain.pop() {
                let parent = parents[index].map_or(Mat4::IDENTITY, |p| composed[p].unwrap());
                composed[index] = Some(parent * scene.objects[index].transform.matrix());
            }
        }
        Ok(Self {
            parents,
            matrices: composed.into_iter().map(Option::unwrap).collect(),
        })
    }
    /// One matrix per document object, in document order.
    pub fn matrices(&self) -> &[Mat4] {
        &self.matrices
    }
    /// The world matrix of the object's parent, or identity for a root.
    pub fn parent(&self, index: usize) -> Mat4 {
        self.parents[index].map_or(Mat4::IDENTITY, |p| self.matrices[p])
    }
}

impl Editor {
    /// Authored world matrices for the current revision, shared until the next transaction.
    pub fn world_transforms(&self) -> Result<Arc<WorldTransforms>> {
        let mut cached = self.world_transforms.borrow_mut();
        if let Some((revision, transforms)) = cached.as_ref()
            && *revision == self.revision
        {
            return Ok(transforms.clone());
        }
        let transforms = Arc::new(WorldTransforms::compose(&self.scene)?);
        *cached = Some((self.revision, transforms.clone()));
        Ok(transforms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_matrices_match_the_validator_in_any_document_order() {
        let mut scene = bozzard_runtime::scene_document().unwrap();
        let ids: Vec<_> = scene.objects.iter().map(|o| o.id.clone()).collect();
        // A deep chain declared child-first, with shear from nonuniform scales.
        for (i, id) in ids.iter().enumerate() {
            let object = scene.objects.iter_mut().find(|o| &o.id == id).unwrap();
            object.transform.rotation_degrees = [i as f32 * 7., 13., -(i as f32) * 3.];
            object.transform.scale = [1. + i as f32 * 0.1, -0.5, 2.];
        }
        let mut chain = Vec::new();
        for i in 0..24 {
            chain.push(Object {
                id: format!("chain-{i}"),
                name: format!("Chain {i}"),
                parent: (i > 0).then(|| format!("chain-{}", i - 1)),
                transform: Transform {
                    translation: [i as f32 * 0.3, 1., -0.25],
                    rotation_degrees: [i as f32 * 11., -(i as f32) * 5., 3.],
                    scale: [1.1, 0.9, if i % 2 == 0 { -1. } else { 1.3 }],
                },
                ..Default::default()
            });
        }
        chain.reverse();
        scene.objects.extend(chain);
        scene.validate().unwrap();
        let reference = scene.global_transforms().unwrap();
        let composed = WorldTransforms::compose(&scene).unwrap();
        for (index, object) in scene.objects.iter().enumerate() {
            assert_eq!(
                composed.matrices()[index].to_cols_array().map(f32::to_bits),
                reference[&object.id].to_cols_array().map(f32::to_bits),
                "{}",
                object.id
            );
            let parent = object
                .parent
                .as_ref()
                .map_or(Mat4::IDENTITY, |p| reference[p]);
            assert_eq!(composed.parent(index), parent);
        }
    }
}
