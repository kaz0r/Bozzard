use crate::{Layout, MAX_BINDINGS, MAX_SOURCE_BYTES, layout};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_KERNEL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextureFormat {
    Rgba8Unorm,
    Rgba16Float,
}
impl TextureFormat {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8Unorm => 4,
            Self::Rgba16Float => 8,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Rgba8Unorm => "rgba8unorm",
            Self::Rgba16Float => "rgba16float",
        }
    }
}
impl std::str::FromStr for TextureFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "rgba8unorm" => Ok(Self::Rgba8Unorm),
            "rgba16float" => Ok(Self::Rgba16Float),
            _ => bail!("compute texture format must be rgba8unorm or rgba16float"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    Uniform(Layout),
    Storage { layout: Layout, writable: bool },
    SampledTexture,
    StorageTexture(TextureFormat),
    Sampler,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub name: String,
    pub group: u32,
    pub binding: u32,
    pub kind: BindingKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPoint {
    pub name: String,
    pub workgroup_size: [u32; 3],
    pub workgroup_bytes: u32,
    pub bindings: Vec<Binding>,
}
impl EntryPoint {
    pub fn binding(&self, name: &str) -> Result<&Binding> {
        self.bindings
            .iter()
            .find(|b| b.name == name)
            .with_context(|| format!("entry '{}' has no binding '{name}'", self.name))
    }
    pub fn parameters(&self) -> Option<&Layout> {
        self.bindings.iter().find_map(|b| {
            if let BindingKind::Uniform(layout) = &b.kind {
                Some(layout)
            } else {
                None
            }
        })
    }
    pub fn pack_parameters(&self, value: &serde_json::Value) -> Result<Vec<u8>> {
        match self.parameters() {
            Some(layout) => layout.pack(value),
            None => {
                ensure!(
                    value.as_object().is_some_and(|map| map.is_empty()),
                    "entry '{}' has no params uniform",
                    self.name
                );
                Ok(Vec::new())
            }
        }
    }
    pub fn groups_for_extent(&self, extent: [u32; 3]) -> Result<[u32; 3]> {
        ensure!(
            extent.iter().all(|v| *v > 0),
            "compute extent must be positive"
        );
        Ok(std::array::from_fn(|i| {
            extent[i].div_ceil(self.workgroup_size[i])
        }))
    }
}

/// A validated immutable source revision. GPU caches can key on identity without hash collisions.
#[derive(Debug)]
pub struct Kernel {
    id: u64,
    source: Arc<str>,
    entries: BTreeMap<String, EntryPoint>,
}
impl Kernel {
    pub fn parse(source: impl Into<Arc<str>>) -> Result<Self> {
        let source = source.into();
        ensure!(
            source.len() <= MAX_SOURCE_BYTES,
            "compute shader exceeds 1 MiB"
        );
        let module = naga::front::wgsl::parse_str(&source)
            .map_err(|e| anyhow::anyhow!("{}", e.emit_to_string(&source)))?;
        ensure!(
            module.overrides.is_empty(),
            "compute pipeline override constants are not supported; use params"
        );
        ensure!(
            module.entry_points.len() <= 16,
            "compute shader exceeds 16 entry points"
        );
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| anyhow::anyhow!("{}", e.emit_to_string(&source)))?;
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx())?;
        let mut entries = BTreeMap::new();
        for (index, entry) in module.entry_points.iter().enumerate() {
            ensure!(
                entry.stage == naga::ShaderStage::Compute,
                "compute assets can only contain compute entry points"
            );
            ensure!(
                entry.workgroup_size.iter().all(|n| *n > 0),
                "invalid compute workgroup size"
            );
            let usage = info.get_entry_point(index);
            ensure!(
                entry.name.len() <= 128,
                "compute entry names must not exceed 128 bytes"
            );
            let mut bindings = Vec::new();
            let mut workgroup_bytes = 0u32;
            for (handle, global) in module.global_variables.iter() {
                if usage[handle].is_empty() {
                    continue;
                }
                if global.space == naga::AddressSpace::WorkGroup {
                    let size = layouter[global.ty]
                        .size
                        .checked_add(15)
                        .context("workgroup memory overflow")?
                        & !15;
                    workgroup_bytes = workgroup_bytes
                        .checked_add(size)
                        .context("workgroup memory overflow")?;
                    continue;
                }
                let Some(binding) = &global.binding else {
                    continue;
                };
                ensure!(
                    binding.group < 4 && binding.binding < 32,
                    "compute bindings require group 0–3 and binding 0–31"
                );
                let name = global.name.clone().context("compute bindings need names")?;
                ensure!(
                    name.len() <= 128,
                    "compute binding names must not exceed 128 bytes"
                );
                let kind = match global.space {
                    naga::AddressSpace::Uniform => {
                        ensure!(
                            name == "params",
                            "the compute uniform block must be named 'params'"
                        );
                        let layout = layout::reflect(&module, &layouter, global.ty, 0)?;
                        ensure!(
                            matches!(layout.shape(), crate::Shape::Struct(_)),
                            "params must be a WGSL struct"
                        );
                        BindingKind::Uniform(layout)
                    }
                    naga::AddressSpace::Storage { access } => BindingKind::Storage {
                        layout: layout::reflect(&module, &layouter, global.ty, 0)?,
                        writable: access.contains(naga::StorageAccess::STORE),
                    },
                    naga::AddressSpace::Handle => match module.types[global.ty].inner {
                        naga::TypeInner::Image {
                            dim: naga::ImageDimension::D2,
                            arrayed: false,
                            class:
                                naga::ImageClass::Sampled {
                                    kind: naga::ScalarKind::Float,
                                    multi: false,
                                },
                        } => BindingKind::SampledTexture,
                        naga::TypeInner::Image {
                            dim: naga::ImageDimension::D2,
                            arrayed: false,
                            class: naga::ImageClass::Storage { format, access },
                        } => {
                            ensure!(
                                access == naga::StorageAccess::STORE,
                                "storage textures must be write-only; use a separate sampled input for feedback"
                            );
                            BindingKind::StorageTexture(match format {
                                naga::StorageFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
                                naga::StorageFormat::Rgba16Float => TextureFormat::Rgba16Float,
                                _ => bail!(
                                    "storage texture format must be rgba8unorm or rgba16float"
                                ),
                            })
                        }
                        naga::TypeInner::Sampler { comparison: false } => BindingKind::Sampler,
                        _ => bail!(
                            "compute bindings support numeric buffers, 2D float textures and regular samplers"
                        ),
                    },
                    _ => bail!("unsupported compute binding address space"),
                };
                bindings.push(Binding {
                    name,
                    group: binding.group,
                    binding: binding.binding,
                    kind,
                });
            }
            ensure!(
                bindings.len() <= MAX_BINDINGS,
                "compute entry exceeds {MAX_BINDINGS} bindings"
            );
            bindings.sort_by_key(|b| (b.group, b.binding));
            entries.insert(
                entry.name.clone(),
                EntryPoint {
                    name: entry.name.clone(),
                    workgroup_size: entry.workgroup_size,
                    workgroup_bytes,
                    bindings,
                },
            );
        }
        ensure!(!entries.is_empty(), "shader has no compute entry point");
        let id = NEXT_KERNEL
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| anyhow::anyhow!("compute shader identities exhausted"))?;
        Ok(Self {
            id,
            source,
            entries,
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn entries(&self) -> impl Iterator<Item = &EntryPoint> {
        self.entries.values()
    }
    pub fn entry(&self, name: &str) -> Result<&EntryPoint> {
        self.entries
            .get(name)
            .with_context(|| format!("compute entry '{name}' does not exist"))
    }
}
