//! Decal: an image projected onto whatever surfaces lie inside a box, along the box's
//! local -Y, like a stamp. Bullet holes, scorch marks, puddles, road markings and signs
//! without touching the meshes they land on.
use super::registry::{Authored, PreviewPolicy};
use crate::{AssetKind, Component, Field, FieldValue, Ui, VectorRole};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Decal {
    pub enabled: bool,
    /// Image asset; empty projects a solid colour.
    pub image: String,
    /// Linear RGB multiplied with the image.
    pub color: [f32; 3],
    pub opacity: f32,
    /// Box size in local units: X and Z span the image, Y is how deep it projects.
    pub size: [f32; 3],
    /// Surfaces tilted further than this from facing the projection are left alone, so a
    /// floor decal does not smear down the sides of a step. Degrees, 0–90.
    pub angle_limit: f32,
    /// Shade with the scene's sun, shadows, ambient and local lights, like a painted
    /// surface. Off: the image keeps its own colour, for emissive signs.
    pub lit: bool,
}
impl Default for Decal {
    fn default() -> Self {
        Self {
            enabled: true,
            image: String::new(),
            color: [1.0; 3],
            opacity: 1.0,
            size: [1.0, 1.0, 1.0],
            angle_limit: 60.0,
            lit: true,
        }
    }
}
impl Component for Decal {
    const NAME: &'static str = "decal";
    const LABEL: &'static str = "Decal";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Projects an image down the object's local -Y onto the surfaces inside its box. Scale or Size sets the area; rotate the object to aim it at walls.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::asset("image", "Image", AssetKind::Image),
            F::vector("color", "Color", VectorRole::Color, 0.01),
            F::range("opacity", "Opacity", 0.01, 0.0, 1.0),
            F::vector_min("size", "Size", VectorRole::Scale, 0.05, 0.001)
                .help("X and Z span the image; Y is how far it projects."),
            F::range("angle_limit", "Angle limit °", 0.5, 0.0, 90.0)
                .help("Surfaces tilted further than this from the projection are skipped."),
            F::bool("lit", "Lit"),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "image" => FieldValue::Asset((!self.image.is_empty()).then(|| self.image.clone())),
            "color" => FieldValue::Vector(self.color),
            "opacity" => FieldValue::Number(self.opacity),
            "size" => FieldValue::Vector(self.size),
            "angle_limit" => FieldValue::Number(self.angle_limit),
            "lit" => FieldValue::Bool(self.lit),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "image" => self.image = value.asset()?.clone().unwrap_or_default(),
            "color" => self.color = value.vector()?,
            "opacity" => self.opacity = value.number()?,
            "size" => self.size = value.vector()?,
            "angle_limit" => self.angle_limit = value.number()?,
            "lit" => self.lit = value.bool()?,
            other => anyhow::bail!("Decal has no field '{other}'"),
        }
        Ok(())
    }
}
impl Authored for Decal {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;
    fn validate(&self) -> Result<()> {
        ensure!(
            self.color
                .iter()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
                && self.opacity.is_finite()
                && (0.0..=1.0).contains(&self.opacity),
            "decal colour must be linear RGB in 0–1 and opacity 0–1"
        );
        ensure!(
            self.size.iter().all(|s| s.is_finite() && *s >= 0.001),
            "decal size must be finite and at least 0.001 on every axis"
        );
        ensure!(
            self.angle_limit.is_finite() && (0.0..=90.0).contains(&self.angle_limit),
            "decal angle limit must be 0–90 degrees"
        );
        Ok(())
    }
}

/// A decal as the renderer draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalView {
    /// Maps the unit box (-0.5..0.5 on every axis) onto the decal's world volume.
    pub volume: Mat4,
    pub image: Option<String>,
    /// Linear RGB and opacity.
    pub color: [f32; 4],
    /// Cosine of the angle limit: the smallest accepted dot product between a surface
    /// normal and the direction the decal faces (its local +Y).
    pub min_facing: f32,
    pub lit: bool,
}
impl Decal {
    pub(crate) fn view(&self, matrix: Mat4) -> Option<DecalView> {
        let volume = matrix * Mat4::from_scale(Vec3::from(self.size));
        (self.enabled && volume.is_finite() && volume.determinant().abs() > 1e-12).then(|| {
            DecalView {
                volume,
                image: (!self.image.is_empty()).then(|| self.image.clone()),
                color: [self.color[0], self.color[1], self.color[2], self.opacity],
                min_facing: self.angle_limit.to_radians().cos(),
                lit: self.lit,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_scale_the_unit_box_and_skip_disabled_or_flat_decals() {
        let decal = Decal {
            image: "crack".into(),
            size: [2.0, 0.5, 4.0],
            opacity: 0.5,
            angle_limit: 60.0,
            ..Default::default()
        };
        let view = decal
            .view(Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(
            view.volume.transform_point3(Vec3::splat(0.5)),
            Vec3::new(2.0, 0.25, 2.0)
        );
        assert_eq!(view.image.as_deref(), Some("crack"));
        assert_eq!(view.color, [1.0, 1.0, 1.0, 0.5]);
        assert!((view.min_facing - 0.5).abs() < 1e-6);
        assert!(
            Decal {
                enabled: false,
                ..Default::default()
            }
            .view(Mat4::IDENTITY)
            .is_none()
        );
        assert!(
            Decal::default()
                .view(Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0)))
                .is_none()
        );
        assert!(
            Decal::default()
                .view(Mat4::IDENTITY)
                .unwrap()
                .image
                .is_none()
        );
    }

    #[test]
    fn out_of_range_settings_are_rejected() {
        for broken in [
            Decal {
                opacity: 1.5,
                ..Default::default()
            },
            Decal {
                size: [1.0, 0.0, 1.0],
                ..Default::default()
            },
            Decal {
                angle_limit: 120.0,
                ..Default::default()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }
}
