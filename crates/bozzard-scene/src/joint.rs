//! Authorable constraints between two Rigidbodies, backed by Rapier impulse joints.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    /// Welds the two bodies into one rigid assembly.
    Fixed,
    /// Hinge: rotation about one shared axis, everything else locked.
    Revolute,
    /// Ball socket: free rotation, positions locked.
    Spherical,
    /// Slider: translation along one shared axis, everything else locked.
    Prismatic,
    /// Chain link: the bodies cannot separate past the maximum distance.
    Rope,
}

/// One constraint on this object, attached to `other` by object ID.
///
/// Both endpoints resolve to their own [Rigidbody](crate::Gravity) root, so a joint authored on a
/// compound child constrains the body that owns it. Anchors and axes are in each body's local space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Joint {
    pub enabled: bool,
    /// The other body. Must be a Rigidbody or any object with an enabled collider.
    pub other: String,
    pub kind: JointKind,
    /// Anchor on this body, in its local space.
    pub anchor: [f32; 3],
    /// Anchor on the other body, in its local space.
    pub other_anchor: [f32; 3],
    /// Hinge or slide axis in this body's local space.
    pub axis: [f32; 3],
    /// The same axis in the other body's local space.
    pub other_axis: [f32; 3],
    /// Hinge angle or slide distance limits. Rope ignores this and reads `max_limit`.
    pub limits: bool,
    pub min_limit: f32,
    pub max_limit: f32,
}
impl Default for Joint {
    fn default() -> Self {
        Self {
            enabled: true,
            other: String::new(),
            kind: JointKind::Fixed,
            anchor: [0.0; 3],
            other_anchor: [0.0; 3],
            axis: [0.0, 1.0, 0.0],
            other_axis: [0.0, 1.0, 0.0],
            limits: false,
            min_limit: -45.0,
            max_limit: 45.0,
        }
    }
}
impl Joint {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.anchor
                .iter()
                .chain(&self.other_anchor)
                .chain(&self.axis)
                .chain(&self.other_axis)
                .all(|v| v.is_finite()),
            "Joint anchors and axes must be finite"
        );
        ensure!(
            !matches!(self.kind, JointKind::Revolute | JointKind::Prismatic)
                || (Vec3::from(self.axis).length_squared() > 1e-9
                    && Vec3::from(self.other_axis).length_squared() > 1e-9),
            "Joint hinge/slide axis must be nonzero"
        );
        ensure!(
            self.min_limit.is_finite()
                && self.max_limit.is_finite()
                && (!self.limits || self.min_limit <= self.max_limit),
            "Joint limits must be finite with Min at or below Max"
        );
        ensure!(
            self.max_limit >= 0.0 || self.kind != JointKind::Rope,
            "Joint rope distance must be nonnegative"
        );
        Ok(())
    }
}
