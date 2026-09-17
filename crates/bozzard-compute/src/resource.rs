use crate::{BindingKind, EntryPoint, Layout, TextureFormat};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// The limits actually enabled by the executor, not the adapter's advertised maximums.
/// `None` is the explicit headless/unavailable policy; no command silently runs on the CPU.
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub backend: Option<std::borrow::Cow<'static, str>>,
    pub device_generation: u64,
    pub max_buffer_bytes: u64,
    pub max_uniform_bytes: u32,
    pub max_texture_dimension: u32,
    pub max_workgroup_size: [u32; 3],
    pub max_workgroup_invocations: u32,
    pub max_workgroup_bytes: u32,
    pub max_workgroups: u32,
    pub max_storage_buffers: u32,
    pub max_storage_textures: u32,
    pub max_sampled_textures: u32,
    pub max_samplers: u32,
}
impl Capabilities {
    pub fn available(&self) -> bool {
        self.backend.is_some()
    }
    pub fn validate_entry(&self, entry: &EntryPoint) -> Result<()> {
        ensure!(
            self.available(),
            "GPU compute is unavailable; guard optional visuals with compute_available() or register a CPU executor"
        );
        ensure!(
            entry
                .workgroup_size
                .iter()
                .zip(self.max_workgroup_size)
                .all(|(n, limit)| *n <= limit),
            "workgroup dimensions exceed enabled device limits"
        );
        let invocations = entry
            .workgroup_size
            .iter()
            .try_fold(1u64, |total, n| total.checked_mul(u64::from(*n)));
        ensure!(
            invocations.is_some_and(|n| n <= u64::from(self.max_workgroup_invocations)),
            "workgroup invocation count exceeds enabled device limits"
        );
        ensure!(
            entry.workgroup_bytes <= self.max_workgroup_bytes,
            "workgroup memory exceeds enabled device limits"
        );
        let mut counts = [0; 4];
        for binding in &entry.bindings {
            match &binding.kind {
                BindingKind::Uniform(layout) => ensure!(
                    layout.minimum_size() <= self.max_uniform_bytes,
                    "params exceed enabled uniform buffer limit"
                ),
                BindingKind::Storage { .. } => counts[0] += 1,
                BindingKind::StorageTexture(_) => counts[1] += 1,
                BindingKind::SampledTexture => counts[2] += 1,
                BindingKind::Sampler => counts[3] += 1,
            }
        }
        ensure!(
            counts
                .into_iter()
                .zip([
                    self.max_storage_buffers,
                    self.max_storage_textures,
                    self.max_sampled_textures,
                    self.max_samplers
                ])
                .all(|(n, limit)| n <= limit),
            "binding count exceeds enabled device limits"
        );
        Ok(())
    }
}

/// A script attachment or compiled-in system. Ownership is checked on every operation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Owner {
    pub object: String,
    pub attachment: usize,
}
impl Owner {
    pub fn new(object: impl Into<String>, attachment: usize) -> Self {
        Self {
            object: object.into(),
            attachment,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    Attachment,
    Scene,
}

/// Opaque, never reused within a world. A new world/device generation invalidates all handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle {
    pub(crate) world: u64,
    pub(crate) serial: u64,
}
impl Handle {
    pub fn world(self) -> u64 {
        self.world
    }
    pub fn serial(self) -> u64 {
        self.serial
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Buffer {
        layout: Arc<Layout>,
        elements: u32,
        bytes: u64,
    },
    /// Linear color, straight alpha, one mip. Use separate input/output for feedback.
    Texture {
        width: u32,
        height: u32,
        format: TextureFormat,
    },
    /// Clamp-to-edge, with either linear or nearest filtering.
    Sampler { linear: bool },
}
impl ResourceKind {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Buffer { bytes, .. } => *bytes,
            Self::Texture {
                width,
                height,
                format,
            } => u64::from(*width) * u64::from(*height) * u64::from(format.bytes_per_pixel()),
            Self::Sampler { .. } => 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Resource {
    pub handle: Handle,
    pub owner: Owner,
    pub scope: Scope,
    pub name: String,
    pub kind: ResourceKind,
}
