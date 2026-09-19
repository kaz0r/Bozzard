//! Portable shared-material definitions and per-instance property overrides.
//! File resolution belongs to the CPU asset loader; these types are headless.
use crate::{Texture, shader_graph::ShaderGraph};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_scale: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
}
impl PropertyOverrides {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.color
                .iter()
                .flatten()
                .chain(self.metallic.iter())
                .chain(self.roughness.iter())
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "material colors and factors must be finite in 0..1"
        );
        ensure!(
            self.uv_scale
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0.001..=1000.).contains(v)),
            "material UV repeat must be finite in 0.001..1000"
        );
        Ok(())
    }
    pub fn apply(&self, values: &mut MaterialValues) {
        if let Some(value) = self.color {
            values.color = value;
        }
        if let Some(value) = self.uv_scale {
            values.uv_scale = value;
        }
        if let Some(value) = self.metallic {
            values.metallic = Some(value);
        }
        if let Some(value) = self.roughness {
            values.roughness = Some(value);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialValues {
    pub color: [f32; 3],
    pub uv_scale: [f32; 2],
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
}
impl Default for MaterialValues {
    fn default() -> Self {
        Self {
            color: [1.; 3],
            uv_scale: [1.; 2],
            metallic: None,
            roughness: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialTexture {
    /// Use the original mesh's material maps.
    #[default]
    Source,
    White,
    Checker,
    Normals,
    ProceduralChecker,
    Toon,
    /// Relative to the material file that declares it, including inherited maps.
    Image(String),
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialShader {
    #[default]
    Inherit,
    Stock,
    Graph(ShaderGraph),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialAsset {
    pub version: u32,
    pub name: String,
    /// Relative source-material path; an instance asset can override any subset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub properties: PropertyOverrides,
    /// None inherits the parent map; Source explicitly returns to mesh maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<MaterialTexture>,
    #[serde(default)]
    pub shader: MaterialShader,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keywords: BTreeMap<String, bool>,
}
impl Default for MaterialAsset {
    fn default() -> Self {
        Self {
            version: 1,
            name: "New Material".into(),
            parent: None,
            properties: Default::default(),
            texture: None,
            shader: Default::default(),
            keywords: BTreeMap::new(),
        }
    }
}
impl MaterialAsset {
    pub const MAX_BYTES: usize = 1024 * 1024;
    pub fn from_json(json: &str) -> Result<Self> {
        ensure!(json.len() <= Self::MAX_BYTES, "material exceeds 1 MiB");
        let material: Self = serde_json::from_str(json).context("parsing material JSON")?;
        material.validate()?;
        Ok(material)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)? + "\n";
        ensure!(json.len() <= Self::MAX_BYTES, "material exceeds 1 MiB");
        Ok(json)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported material version");
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "material needs a name (1–128 bytes)"
        );
        self.properties.validate()?;
        if let Some(parent) = &self.parent {
            validate_relative_path(parent)?;
        }
        if let Some(MaterialTexture::Image(path)) = &self.texture {
            validate_relative_path(path)?;
        }
        if let MaterialShader::Graph(graph) = &self.shader {
            graph.validate()?;
        }
        validate_keywords(&self.keywords)?;
        // Declared names are checked after resolving the inherited graph.
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialInstance {
    /// A material asset ID in the scene catalog.
    pub asset: String,
    #[serde(default)]
    pub properties: PropertyOverrides,
    /// Optional per-object texture asset or built-in effect; None inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Texture>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keywords: BTreeMap<String, bool>,
}
impl MaterialInstance {
    pub fn new(asset: String) -> Self {
        Self {
            asset,
            properties: Default::default(),
            texture: None,
            keywords: BTreeMap::new(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.asset.is_empty() && self.asset.len() <= 128,
            "material instance needs an asset ID"
        );
        self.properties.validate()?;
        validate_keywords(&self.keywords)
    }
}
fn validate_keywords(keywords: &BTreeMap<String, bool>) -> Result<()> {
    ensure!(
        keywords.len() <= 8
            && keywords
                .keys()
                .all(|key| crate::shader_graph::valid_keyword(key)),
        "material supports at most eight valid shader keywords"
    );
    Ok(())
}
pub fn validate_relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && path.len() <= 1024
            && !path.chars().any(char::is_control)
            && !path.starts_with('/')
            && !path.starts_with('\\')
            && !path.contains(':')
            && !path.contains('\\'),
        "material dependencies require portable relative paths"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_properties_inherit_independently_and_roundtrip() {
        let mut values = MaterialValues::default();
        PropertyOverrides {
            color: Some([0.2, 0.4, 0.6]),
            metallic: Some(0.8),
            ..Default::default()
        }
        .apply(&mut values);
        PropertyOverrides {
            roughness: Some(0.3),
            ..Default::default()
        }
        .apply(&mut values);
        assert_eq!(values.color, [0.2, 0.4, 0.6]);
        assert_eq!((values.metallic, values.roughness), (Some(0.8), Some(0.3)));
        let definition = MaterialAsset {
            parent: Some("../base.material.json".into()),
            texture: Some(MaterialTexture::Image("maps/detail.png".into())),
            ..Default::default()
        };
        assert_eq!(
            MaterialAsset::from_json(&definition.to_json().unwrap()).unwrap(),
            definition
        );
        for invalid in [
            "/absolute",
            "C:/absolute",
            "https://host/map",
            "a\\b",
            "bad\npath",
            "",
        ] {
            assert!(validate_relative_path(invalid).is_err());
        }
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(
                PropertyOverrides {
                    roughness: Some(invalid),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
