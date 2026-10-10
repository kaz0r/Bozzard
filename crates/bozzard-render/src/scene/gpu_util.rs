//! Construction-time wgpu helpers the full-screen passes share.
use super::*;

/// An HDR (RGBA16F) target that a pass renders and a later pass samples.
pub(super) fn color_texture(gpu: &Gpu, size: [u32; 2], label: &str) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// A full-screen-triangle pass: the shader's `vs_main`, then `entry` writing one unblended
/// target per format.
pub(super) fn fullscreen_pipeline(
    gpu: &Gpu,
    label: &str,
    layout: Option<&wgpu::PipelineLayout>,
    shader: &wgpu::ShaderModule,
    entry: &str,
    formats: &[wgpu::TextureFormat],
) -> wgpu::RenderPipeline {
    let targets: Vec<_> = formats
        .iter()
        .map(|&format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
        .collect();
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

/// Values for the two most recent input keys. Temporal history alternates its
/// output between two targets, so each consumer keeps a binding for both.
pub(super) struct Recent<K, V> {
    entries: [Option<(K, V)>; 2],
    current: usize,
}
impl<K, V> Default for Recent<K, V> {
    fn default() -> Self {
        Self {
            entries: [None, None],
            current: 0,
        }
    }
}
impl<K: PartialEq, V> Recent<K, V> {
    /// Select the value for `key`, creating it in place of the older entry.
    /// A repeated key releases the other entry, so stable inputs do not keep
    /// a former source alive. Returns whether the value was created.
    pub fn select(&mut self, key: K, create: impl FnOnce(&K) -> V) -> bool {
        if let Some(slot) = (0..2).find(|&slot| {
            self.entries[slot]
                .as_ref()
                .is_some_and(|(existing, _)| *existing == key)
        }) {
            if slot == self.current {
                self.entries[1 - slot] = None;
            }
            self.current = slot;
            return false;
        }
        if self.entries[self.current].is_some() {
            self.current = 1 - self.current;
        }
        let value = create(&key);
        self.entries[self.current] = Some((key, value));
        true
    }
    pub fn current(&self) -> Option<&V> {
        self.entries[self.current].as_ref().map(|(_, value)| value)
    }
    pub fn clear(&mut self) {
        self.entries = [None, None];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_values_alternate_and_replace_the_older_entry() {
        let mut recent = Recent::default();
        let mut created = Vec::new();
        for key in [1, 2, 1, 2, 1, 3, 1, 3, 2] {
            if recent.select(key, |k| k * 10) {
                created.push(key);
            }
            assert_eq!(recent.current(), Some(&(key * 10)));
        }
        assert_eq!(created, [1, 2, 3, 2]);
        assert!(!recent.select(2, |k| k * 10));
        assert!(recent.select(3, |k| k * 10), "a repeated key released 3");
        recent.clear();
        assert_eq!(recent.current(), None);
    }
}
