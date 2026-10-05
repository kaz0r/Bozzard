//! Pass-local resource identity cache. Buffer slice offsets are part of state;
//! underlying shared allocations compare equal even through cloned handles.
#[derive(Default)]
pub(super) struct DrawState {
    pipeline: Option<wgpu::RenderPipeline>,
    groups: [Option<wgpu::BindGroup>; 4],
    vertices: [Option<(wgpu::Buffer, u64)>; 3],
    indices: Option<wgpu::Buffer>,
    pub counts: Counts,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Counts {
    pub pipelines: usize,
    pub groups: usize,
    pub vertices: usize,
    pub indices: usize,
}
impl DrawState {
    /// Bundles reset WebGPU render state; callers must reset this cache too.
    pub fn reset(&mut self) {
        self.pipeline = None;
        self.groups.fill(None);
        self.vertices.fill(None);
        self.indices = None;
    }
    pub fn pipeline(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        value: &wgpu::RenderPipeline,
        cache: bool,
    ) -> bool {
        if cache && self.pipeline.as_ref() == Some(value) {
            return false;
        }
        pass.set_pipeline(value);
        self.pipeline = Some(value.clone());
        self.counts.pipelines += 1;
        true
    }
    pub fn group(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: usize,
        value: &wgpu::BindGroup,
        cache: bool,
    ) {
        if !cache || self.groups[slot].as_ref() != Some(value) {
            pass.set_bind_group(slot as u32, value, &[]);
            self.groups[slot] = Some(value.clone());
            self.counts.groups += 1;
        }
    }
    pub fn vertex(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: usize,
        value: &wgpu::Buffer,
        offset: u64,
        cache: bool,
    ) {
        if !cache
            || self.vertices[slot]
                .as_ref()
                .is_none_or(|(old, start)| old != value || *start != offset)
        {
            pass.set_vertex_buffer(slot as u32, value.slice(offset..));
            self.vertices[slot] = Some((value.clone(), offset));
            self.counts.vertices += 1;
        }
    }
    pub fn index(&mut self, pass: &mut wgpu::RenderPass<'_>, value: &wgpu::Buffer, cache: bool) {
        if !cache || self.indices.as_ref() != Some(value) {
            pass.set_index_buffer(value.slice(..), wgpu::IndexFormat::Uint32);
            self.indices = Some(value.clone());
            self.counts.indices += 1;
        }
    }
}
