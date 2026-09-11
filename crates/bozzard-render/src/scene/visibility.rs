use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub scene_items: usize,
    pub surfaces: usize,
    pub visible_surfaces: usize,
    pub culled_surfaces: usize,
    pub color_triangles: u64,
    pub shadow_draws: usize,
    pub shadow_triangles: u64,
    /// Color-pass mesh pipeline binds; excludes sky, shadow and display passes.
    pub pipeline_binds: usize,
    /// CPU work only, including command submission. Not GPU execution or FPS.
    pub cpu_ms: f64,
    pub prepare_ms: f64,
    pub encode_ms: f64,
    pub submit_ms: f64,
}
/// Reject only when every transformed AABB corner is outside the same homogeneous
/// clip plane. No perspective divide: handles near-plane crossings and negative w.
pub(super) fn visible(bounds: [Vec3; 2], mvp: Mat4) -> bool {
    let corners: [glam::Vec4; 8] = std::array::from_fn(|i| {
        mvp * Vec3::new(
            bounds[i & 1].x,
            bounds[(i >> 1) & 1].y,
            bounds[(i >> 2) & 1].z,
        )
        .extend(1.)
    });
    !(0..6).any(|plane| {
        corners.iter().all(|q| {
            let distance = match plane {
                0 => q.w + q.x,
                1 => q.w - q.x,
                2 => q.w + q.y,
                3 => q.w - q.y,
                4 => q.z,
                _ => q.w - q.z,
            };
            let tolerance = q.abs().max_element().max(1.) * 1e-6;
            distance < -tolerance
        })
    })
}
impl SceneRenderer {
    pub fn frame_stats(&self) -> FrameStats {
        self.stats
    }
    /// Diagnostic switches permit output/performance comparison with the reference path.
    pub fn set_culling_enabled(&mut self, enabled: bool) {
        self.culling = enabled;
    }
    pub fn set_state_caching_enabled(&mut self, enabled: bool) {
        self.state_caching = enabled;
    }
    pub(super) fn visibility(&self, scene: &RenderScene, draws: &[PreparedDraw]) -> Vec<bool> {
        draws
            .iter()
            .map(|d| {
                !self.culling
                    || visible(
                        self.mesh_for(&d.object.mesh).bounds,
                        scene.view_projection * d.object.model,
                    )
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conservative_clip_planes_transform_and_camera_crossings() {
        let b = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        let ortho = glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.);
        assert!(visible(
            b,
            ortho * Mat4::from_translation(Vec3::new(0., 0., -2.))
        ));
        for p in [
            Vec3::new(2., 0., -2.),
            Vec3::new(-2., 0., -2.),
            Vec3::new(0., 2., -2.),
            Vec3::new(0., -2., -2.),
            Vec3::new(0., 0., 1.),
            Vec3::new(0., 0., -11.),
        ] {
            assert!(!visible(b, ortho * Mat4::from_translation(p)), "{p:?}");
        }
        assert!(visible(
            b,
            ortho * Mat4::from_translation(Vec3::new(1.5, 0., -2.))
        ));
        assert!(visible(
            b,
            ortho
                * Mat4::from_translation(Vec3::new(1.2, 0., -2.))
                * Mat4::from_rotation_z(0.6)
                * Mat4::from_scale(Vec3::new(-2., 0.2, 1.))
        ));
        let perspective = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 10.);
        assert!(visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(0., 0., -0.2))
        ));
        assert!(!visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(0., 0., 2.))
        ));
        assert!(!visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(20., 0., -2.))
        ));
    }
}
