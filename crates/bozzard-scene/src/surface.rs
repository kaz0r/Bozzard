use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Per-instance surface edits; source geometry/materials remain immutable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceMaterialOverride {
    pub surface: u32,
    /// Importer's deterministic geometry/source signature, not an asset path.
    pub source: String,
    /// Delta in model space, rotating/scaling about the source surface bounds center.
    #[serde(default)]
    pub transform: crate::Transform,
    /// None inherits the model/source texture; Some(White) explicitly removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<crate::Texture>,
    #[serde(default = "unit_uv")]
    pub uv_scale: [f32; 2],
    #[serde(default = "white")]
    pub tint: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
}

fn unit_uv() -> [f32; 2] {
    [1.0; 2]
}
fn white() -> [f32; 3] {
    [1.0; 3]
}
impl SurfaceMaterialOverride {
    pub fn inherited(surface: u32, source: String) -> Self {
        Self {
            surface,
            source,
            transform: Default::default(),
            texture: None,
            uv_scale: unit_uv(),
            tint: white(),
            metallic: None,
            roughness: None,
        }
    }
    pub fn is_inherited(&self) -> bool {
        self.transform == crate::Transform::default()
            && self.texture.is_none()
            && self.uv_scale == unit_uv()
            && self.tint == white()
            && self.metallic.is_none()
            && self.roughness.is_none()
    }
    pub fn matrix(&self, pivot: glam::Vec3) -> glam::Mat4 {
        glam::Mat4::from_translation(pivot)
            * self.transform.matrix()
            * glam::Mat4::from_translation(-pivot)
    }
    pub fn validate(&self) -> Result<()> {
        self.transform.validate()?;
        ensure!(
            self.transform.matrix().is_finite() && self.transform.matrix().inverse().is_finite(),
            "invalid surface transform"
        );
        ensure!(
            self.uv_scale.iter().all(|v| v.is_finite() && *v > 0.0),
            "surface UV scale must be finite and positive"
        );
        ensure!(
            self.surface < 4096,
            "material override surface exceeds import limit"
        );
        ensure!(
            self.source.len() == 16
                && self
                    .source
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "invalid material override source signature"
        );
        ensure!(
            self.tint
                .iter()
                .chain(self.metallic.iter())
                .chain(self.roughness.iter())
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "material override factors must be finite and in 0..1"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    #[test]
    fn optional_overrides_roundtrip_and_reject_invalid_scene_data() {
        let mut doc = serde_json::json!({"version":1,"name":"Materials","views":{},"assets":{"model":{"kind":"mesh","path":"model.gltf"}},"objects":[{"id":"model","name":"Model","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":{"asset":"model"},"texture":"white","color":[1,1,1],"uv_scale":[1,1]}}]});
        let legacy = Scene::from_json(&doc.to_string()).unwrap();
        assert!(
            legacy.objects[0]
                .drawable
                .as_ref()
                .unwrap()
                .material_overrides
                .is_empty()
        );
        let value = SurfaceMaterialOverride {
            surface: 1,
            source: "0123456789abcdef".into(),
            transform: Default::default(),
            texture: None,
            uv_scale: [1.; 2],
            tint: [0.25, 0.5, 1.0],
            metallic: Some(0.7),
            roughness: Some(0.2),
        };
        doc["objects"][0]["drawable"]["material_overrides"] = serde_json::json!([value]);
        let scene = Scene::from_json(&doc.to_string()).unwrap();
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
        let invalid = |values: serde_json::Value| {
            let mut bad = doc.clone();
            bad["objects"][0]["drawable"]["material_overrides"] = values;
            assert!(Scene::from_json(&bad.to_string()).is_err());
        };
        invalid(serde_json::json!([value, value]));
        for field in ["metallic", "roughness"] {
            let mut bad = serde_json::json!([value]);
            bad[0][field] = 1.1.into();
            invalid(bad);
        }
        let mut bad = value.clone();
        bad.tint[0] = f32::NAN;
        assert!(bad.validate().is_err());
        bad.tint[0] = 1.;
        bad.source = "malformed".into();
        assert!(bad.validate().is_err());
        bad.source = value.source;
        bad.surface = 4096;
        assert!(bad.validate().is_err());
        doc["objects"][0]["drawable"]["mesh"] = "cube".into();
        assert!(Scene::from_json(&doc.to_string()).is_err());
    }
}
