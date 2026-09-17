//! Bounded CPU reservations and an ordered, transactional submission boundary.
use crate::{
    BindingKind, Capabilities, Handle, Kernel, Layout, Owner, Resource, ResourceKind, Scope,
    TextureFormat,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

pub const MAX_RESOURCES: usize = 256;
pub const MAX_RESOURCE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_COMMANDS: usize = 1024;
pub const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_JOBS: usize = 256;
pub const MAX_READBACKS: usize = 8;
pub const MAX_READBACK_BYTES: u64 = 1024 * 1024;
pub const MAX_SUBMISSIONS: usize = 32;
const MAX_NAME_BYTES: usize = 128;
static NEXT_WORLD: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticket {
    world: u64,
    serial: u64,
}
impl Ticket {
    pub fn world(self) -> u64 {
        self.world
    }
    pub fn serial(self) -> u64 {
        self.serial
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Submitted,
    Complete,
    Failed(String),
    Cancelled,
}
impl JobState {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Complete | Self::Failed(_) | Self::Cancelled)
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Submitted => "submitted",
            Self::Complete => "complete",
            Self::Failed(_) => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Job {
    pub ticket: Ticket,
    pub owner: Owner,
    pub label: String,
    pub tick: u64,
    pub state: JobState,
    /// Readbacks retain their result until taken or cancelled; completed dispatch receipts
    /// may be evicted once the job budget is needed by later commands.
    pub readback: bool,
    layout: Option<Arc<Layout>>,
    bytes: Option<Arc<[u8]>>,
}

/// A call from either Rust or a scripting wrapper. Parameter values are packed immediately;
/// changing the caller's map after this call cannot alter an accepted dispatch.
pub struct Dispatch<'a> {
    pub asset: &'a str,
    pub kernel: Arc<Kernel>,
    pub entry: &'a str,
    pub bindings: &'a BTreeMap<String, Handle>,
    pub params: &'a serde_json::Value,
    pub groups: [u32; 3],
}

#[derive(Clone, Debug)]
pub enum Command {
    Create(Arc<Resource>),
    Write {
        resource: Arc<Resource>,
        offset: u64,
        bytes: Arc<[u8]>,
    },
    Dispatch {
        ticket: Ticket,
        asset: String,
        kernel: Arc<Kernel>,
        entry: String,
        /// Non-uniform resources in reflected (group, binding) order.
        resources: Vec<Arc<Resource>>,
        params: Arc<[u8]>,
        groups: [u32; 3],
    },
    Readback {
        ticket: Ticket,
        resource: Arc<Resource>,
        offset: u64,
        bytes: u64,
    },
    Release(Arc<Resource>),
}
impl Command {
    fn ticket(&self) -> Option<Ticket> {
        match self {
            Self::Dispatch { ticket, .. } | Self::Readback { ticket, .. } => Some(*ticket),
            _ => None,
        }
    }
    fn upload_bytes(&self) -> usize {
        match self {
            Self::Write { bytes, .. } => bytes.len(),
            Self::Dispatch { params, .. } => params.len(),
            _ => 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Request {
    pub sequence: u64,
    pub tick: u64,
    pub command: Command,
}

/// A borrowed batch cannot outlive the submission closure or race cancellation. It is consumed
/// only when the closure reports an actual queue submission. `Deferred` preserves every request.
pub struct Batch<'a> {
    pub world: u64,
    pub serial: u64,
    pub requests: &'a [Request],
}
pub enum Submission {
    Submitted,
    Deferred,
    Rejected(String),
}

enum Completion {
    Submission(u64),
    Readback(Ticket, std::result::Result<Arc<[u8]>, String>),
    DeviceFailure(String),
}
/// GPU callbacks only post bounded messages. Simulation state changes at `begin_tick`.
#[derive(Clone)]
pub struct CompletionSink(mpsc::SyncSender<Completion>);
impl CompletionSink {
    /// Call once after this batch's work completes. Readbacks finish separately when mapped.
    pub fn submitted_work_done(&self, serial: u64) {
        let _ = self.0.try_send(Completion::Submission(serial));
    }
    pub fn readback_done(&self, ticket: Ticket, result: std::result::Result<Arc<[u8]>, String>) {
        let _ = self.0.try_send(Completion::Readback(ticket, result));
    }
    pub fn device_failed(&self, reason: String) {
        let _ = self.0.try_send(Completion::DeviceFailure(reason));
    }
}
#[derive(Default, Clone, Copy, Debug, serde::Serialize)]
pub struct Statistics {
    pub resources: usize,
    pub resource_bytes: u64,
    pub queued_commands: usize,
    pub queued_upload_bytes: usize,
    pub jobs: usize,
    pub pending_readbacks: usize,
    pub in_flight_submissions: usize,
    pub submitted_dispatches: u64,
    pub uploaded_bytes: u64,
    pub readback_bytes: u64,
}
struct ResourceState {
    resource: Arc<Resource>,
    retiring: bool,
    error: Option<String>,
}
struct Pending {
    jobs: Vec<Ticket>,
    releases: Vec<Handle>,
}

/// Scene-local state with no GPU or ECS dependency. Empty runtimes allocate no queues, channels,
/// resources, or result storage. Executors and gameplay systems share this exact protocol.
pub struct Runtime {
    world: u64,
    serial: u64,
    tick: u64,
    capabilities: Capabilities,
    resources: BTreeMap<Handle, ResourceState>,
    names: BTreeMap<(Option<Owner>, String), Handle>,
    jobs: BTreeMap<Ticket, Job>,
    readbacks: BTreeSet<Ticket>,
    requests: Vec<Request>,
    pending: BTreeMap<u64, Pending>,
    channel: Option<(CompletionSink, mpsc::Receiver<Completion>)>,
    stats: Statistics,
}
impl Default for Runtime {
    fn default() -> Self {
        Self::new(Capabilities::default())
    }
}
impl Runtime {
    pub fn new(capabilities: Capabilities) -> Self {
        // Exhaustion after 2^64 new worlds is not recoverable; never wrap into a live identity.
        let world = NEXT_WORLD
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("compute world identities exhausted");
        Self {
            world,
            serial: 0,
            tick: 0,
            capabilities,
            resources: BTreeMap::new(),
            names: BTreeMap::new(),
            jobs: BTreeMap::new(),
            readbacks: BTreeSet::new(),
            requests: Vec::new(),
            pending: BTreeMap::new(),
            channel: None,
            stats: Statistics::default(),
        }
    }
    pub fn world(&self) -> u64 {
        self.world
    }
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    /// Changing device invalidates logical handles, jobs and callbacks from the previous world.
    pub fn set_capabilities(&mut self, capabilities: Capabilities) {
        if self.capabilities.device_generation != capabilities.device_generation
            || self.capabilities.available() != capabilities.available()
        {
            *self = Self::new(capabilities);
        } else {
            self.capabilities = capabilities;
        }
    }
    pub fn reset(&mut self) {
        *self = Self::new(self.capabilities.clone());
    }
    pub fn statistics(&self) -> Statistics {
        Statistics {
            resources: self.resources.len(),
            queued_commands: self.requests.len(),
            jobs: self.jobs.len(),
            pending_readbacks: self.readbacks.len(),
            in_flight_submissions: self.pending.len(),
            ..self.stats
        }
    }
    pub fn resources(&self) -> impl Iterator<Item = (&Resource, bool, Option<&str>)> {
        self.resources.values().map(|state| {
            (
                state.resource.as_ref(),
                state.retiring,
                state.error.as_deref(),
            )
        })
    }
    pub fn jobs(&self) -> impl Iterator<Item = &Job> {
        self.jobs.values()
    }
    fn next_serial(&mut self) -> Result<u64> {
        self.serial = self
            .serial
            .checked_add(1)
            .context("compute request identities exhausted")?;
        Ok(self.serial)
    }
    fn room(&self, uploads: usize) -> Result<()> {
        ensure!(
            self.capabilities.available(),
            "GPU compute is unavailable; guard optional visuals with compute_available() or register a CPU executor"
        );
        ensure!(
            self.requests.len() < MAX_COMMANDS,
            "compute backpressure: command queue is full"
        );
        ensure!(
            uploads <= MAX_UPLOAD_BYTES.saturating_sub(self.stats.queued_upload_bytes),
            "compute backpressure: upload budget is full"
        );
        Ok(())
    }
    fn enqueue(&mut self, command: Command) -> Result<()> {
        let sequence = self.next_serial()?;
        self.stats.queued_upload_bytes += command.upload_bytes();
        self.requests.push(Request {
            sequence,
            tick: self.tick,
            command,
        });
        Ok(())
    }
    fn name_key(owner: &Owner, scope: Scope, name: &str) -> (Option<Owner>, String) {
        (
            match scope {
                Scope::Attachment => Some(owner.clone()),
                Scope::Scene => None,
            },
            name.to_owned(),
        )
    }
    pub fn find(&self, owner: &Owner, scope: Scope, name: &str) -> Result<Handle> {
        self.names
            .get(&Self::name_key(owner, scope, name))
            .copied()
            .with_context(|| format!("unknown compute resource '{name}' in {scope:?} scope"))
    }
    pub fn resource(&self, owner: &Owner, handle: Handle) -> Result<&Resource> {
        Ok(self.resource_arc(owner, handle)?.as_ref())
    }
    fn resource_arc(&self, owner: &Owner, handle: Handle) -> Result<&Arc<Resource>> {
        ensure!(
            handle.world == self.world,
            "stale or cross-world compute handle"
        );
        let state = self
            .resources
            .get(&handle)
            .context("released compute handle")?;
        ensure!(!state.retiring, "released compute handle");
        if let Some(error) = &state.error {
            bail!("compute resource '{}': {error}", state.resource.name);
        }
        ensure!(
            state.resource.scope == Scope::Scene || state.resource.owner == *owner,
            "compute handle belongs to another attachment; use explicit scene scope to share resources"
        );
        Ok(&state.resource)
    }
    pub fn create_buffer(
        &mut self,
        owner: &Owner,
        scope: Scope,
        name: &str,
        layout: Arc<Layout>,
        elements: u32,
    ) -> Result<Handle> {
        let bytes = layout.buffer_size(elements)? as u64;
        self.room(0)?;
        ensure!(
            bytes <= self.capabilities.max_buffer_bytes,
            "compute buffer exceeds enabled device limit"
        );
        self.create(
            owner,
            scope,
            name,
            ResourceKind::Buffer {
                layout,
                elements,
                bytes,
            },
        )
    }
    pub fn create_texture(
        &mut self,
        owner: &Owner,
        scope: Scope,
        name: &str,
        width: u32,
        height: u32,
        format: TextureFormat,
    ) -> Result<Handle> {
        self.room(0)?;
        ensure!(
            width > 0
                && height > 0
                && width <= self.capabilities.max_texture_dimension
                && height <= self.capabilities.max_texture_dimension,
            "compute texture dimensions exceed enabled device limit"
        );
        self.create(
            owner,
            scope,
            name,
            ResourceKind::Texture {
                width,
                height,
                format,
            },
        )
    }
    pub fn create_sampler(
        &mut self,
        owner: &Owner,
        scope: Scope,
        name: &str,
        linear: bool,
    ) -> Result<Handle> {
        self.create(owner, scope, name, ResourceKind::Sampler { linear })
    }
    fn create(
        &mut self,
        owner: &Owner,
        scope: Scope,
        name: &str,
        kind: ResourceKind,
    ) -> Result<Handle> {
        self.room(0)?;
        ensure!(
            !name.is_empty()
                && name.len() <= MAX_NAME_BYTES
                && owner.object.len() <= MAX_NAME_BYTES,
            "compute resource and owner names must be 1–128 bytes"
        );
        let key = Self::name_key(owner, scope, name);
        ensure!(
            !self.names.contains_key(&key),
            "compute resource '{name}' already exists; release it before replacing it"
        );
        ensure!(
            self.resources.len() < MAX_RESOURCES,
            "compute backpressure: resource count limit reached"
        );
        ensure!(
            kind.bytes() <= MAX_RESOURCE_BYTES.saturating_sub(self.stats.resource_bytes),
            "compute backpressure: resource memory budget exceeded"
        );
        let handle = Handle {
            world: self.world,
            serial: self.next_serial()?,
        };
        let resource = Arc::new(Resource {
            handle,
            owner: owner.clone(),
            scope,
            name: name.to_owned(),
            kind,
        });
        self.enqueue(Command::Create(resource.clone()))?;
        self.stats.resource_bytes += resource.kind.bytes();
        self.resources.insert(
            handle,
            ResourceState {
                resource,
                retiring: false,
                error: None,
            },
        );
        self.names.insert(key, handle);
        Ok(handle)
    }
    pub fn write(
        &mut self,
        owner: &Owner,
        handle: Handle,
        value: &serde_json::Value,
    ) -> Result<()> {
        let resource = self.resource_arc(owner, handle)?.clone();
        let ResourceKind::Buffer { layout, bytes, .. } = &resource.kind else {
            bail!("compute_write requires a buffer");
        };
        self.room(usize::try_from(*bytes)?)?;
        let packed = layout.pack(value)?;
        ensure!(
            packed.len() as u64 == *bytes,
            "write size does not match reserved buffer; use write_range for an array subrange"
        );
        self.enqueue(Command::Write {
            resource,
            offset: 0,
            bytes: packed.into(),
        })
    }
    pub fn write_range(
        &mut self,
        owner: &Owner,
        handle: Handle,
        first: u32,
        values: &serde_json::Value,
    ) -> Result<()> {
        let resource = self.resource_arc(owner, handle)?.clone();
        let ResourceKind::Buffer {
            layout, elements, ..
        } = &resource.kind
        else {
            bail!("write_range requires an array buffer");
        };
        let count = u32::try_from(
            values
                .as_array()
                .context("write_range expects an array")?
                .len(),
        )?;
        let (offset, slice) = layout.array_range(*elements, first, count)?;
        self.room(slice.minimum_size() as usize)?;
        self.enqueue(Command::Write {
            resource,
            offset,
            bytes: slice.pack(values)?.into(),
        })
    }
    fn job_room(&mut self) -> Result<()> {
        if self.jobs.len() >= MAX_JOBS {
            let removable = self
                .jobs
                .iter()
                .find(|(_, job)| !job.readback && job.state.terminal())
                .map(|(ticket, _)| *ticket);
            if let Some(ticket) = removable {
                self.jobs.remove(&ticket);
            }
        }
        ensure!(
            self.jobs.len() < MAX_JOBS,
            "compute backpressure: outstanding job budget is full; take readback results and retry later"
        );
        Ok(())
    }
    fn new_job(
        &mut self,
        owner: &Owner,
        label: String,
        layout: Option<Arc<Layout>>,
    ) -> Result<Ticket> {
        self.job_room()?;
        let ticket = Ticket {
            world: self.world,
            serial: self.next_serial()?,
        };
        self.jobs.insert(
            ticket,
            Job {
                ticket,
                owner: owner.clone(),
                label,
                tick: self.tick,
                state: JobState::Queued,
                readback: layout.is_some(),
                layout,
                bytes: None,
            },
        );
        Ok(ticket)
    }
    pub fn dispatch(&mut self, owner: &Owner, call: Dispatch<'_>) -> Result<Ticket> {
        self.room(0)?;
        ensure!(
            !call.asset.is_empty()
                && call.asset.len() <= MAX_NAME_BYTES
                && !owner.object.is_empty()
                && owner.object.len() <= MAX_NAME_BYTES,
            "compute asset and owner IDs must be 1–128 bytes"
        );
        let entry = call.kernel.entry(call.entry)?;
        self.capabilities.validate_entry(entry)?;
        ensure!(
            call.groups
                .iter()
                .all(|n| *n > 0 && *n <= self.capabilities.max_workgroups),
            "dispatch dimensions exceed enabled device limits"
        );
        let expected = entry
            .bindings
            .iter()
            .filter(|b| !matches!(b.kind, BindingKind::Uniform(_)))
            .count();
        ensure!(
            call.bindings.len() == expected,
            "compute bindings do not match entry '{}': expected {expected} resources",
            call.entry
        );
        let mut resources = Vec::with_capacity(expected);
        let mut aliases = BTreeMap::new();
        for binding in &entry.bindings {
            if matches!(binding.kind, BindingKind::Uniform(_)) {
                continue;
            }
            let handle = call
                .bindings
                .get(&binding.name)
                .with_context(|| format!("missing compute binding '{}'", binding.name))?;
            let resource = self.resource_arc(owner, *handle)?;
            let writable = match (&binding.kind, &resource.kind) {
                (
                    BindingKind::Storage {
                        layout: expected,
                        writable,
                    },
                    ResourceKind::Buffer { layout, .. },
                ) => {
                    ensure!(
                        layout.as_ref() == expected,
                        "buffer layout for '{}' changed or does not match; recreate this resource",
                        binding.name
                    );
                    *writable
                }
                (BindingKind::SampledTexture, ResourceKind::Texture { .. })
                | (BindingKind::Sampler, ResourceKind::Sampler { .. }) => false,
                (BindingKind::StorageTexture(expected), ResourceKind::Texture { format, .. }) => {
                    ensure!(
                        format == expected,
                        "storage texture format does not match '{}'",
                        binding.name
                    );
                    true
                }
                _ => bail!(
                    "resource kind does not match compute binding '{}'",
                    binding.name
                ),
            };
            if let Some(previous) = aliases.insert(*handle, writable) {
                ensure!(
                    !writable && !previous,
                    "a writable compute resource cannot alias another binding; use ping-pong resources"
                );
            }
            resources.push(resource.clone());
        }
        let params: Arc<[u8]> = entry.pack_parameters(call.params)?.into();
        self.room(params.len())?;
        let ticket = self.new_job(owner, format!("{}::{}", call.asset, call.entry), None)?;
        self.enqueue(Command::Dispatch {
            ticket,
            asset: call.asset.to_owned(),
            kernel: call.kernel,
            entry: call.entry.to_owned(),
            resources,
            params,
            groups: call.groups,
        })?;
        Ok(ticket)
    }
    pub fn readback(&mut self, owner: &Owner, handle: Handle) -> Result<Ticket> {
        let resource = self.resource_arc(owner, handle)?.clone();
        let ResourceKind::Buffer { layout, bytes, .. } = &resource.kind else {
            bail!("readback requires a buffer");
        };
        self.queue_readback(owner, resource.clone(), 0, *bytes, layout.clone())
    }
    pub fn readback_range(
        &mut self,
        owner: &Owner,
        handle: Handle,
        first: u32,
        count: u32,
    ) -> Result<Ticket> {
        let resource = self.resource_arc(owner, handle)?.clone();
        let ResourceKind::Buffer {
            layout, elements, ..
        } = &resource.kind
        else {
            bail!("readback_range requires an array buffer");
        };
        let (offset, slice) = layout.array_range(*elements, first, count)?;
        self.queue_readback(
            owner,
            resource,
            offset,
            u64::from(slice.minimum_size()),
            Arc::new(slice),
        )
    }
    fn queue_readback(
        &mut self,
        owner: &Owner,
        resource: Arc<Resource>,
        offset: u64,
        bytes: u64,
        layout: Arc<Layout>,
    ) -> Result<Ticket> {
        self.room(0)?;
        ensure!(
            bytes <= MAX_READBACK_BYTES,
            "readback exceeds 1 MiB; request a bounded array range"
        );
        ensure!(
            self.readbacks.len() < MAX_READBACKS,
            "compute backpressure: all eight readback slots are occupied"
        );
        let ticket = self.new_job(owner, format!("readback {}", resource.name), Some(layout))?;
        self.enqueue(Command::Readback {
            ticket,
            resource,
            offset,
            bytes,
        })?;
        self.readbacks.insert(ticket);
        Ok(ticket)
    }
    pub fn job(&self, owner: &Owner, ticket: Ticket) -> Result<&Job> {
        ensure!(
            ticket.world == self.world,
            "stale or cross-world compute ticket"
        );
        let job = self
            .jobs
            .get(&ticket)
            .context("unknown or expired compute ticket")?;
        ensure!(
            job.owner == *owner,
            "compute ticket belongs to another attachment"
        );
        Ok(job)
    }
    pub fn take_result(
        &mut self,
        owner: &Owner,
        ticket: Ticket,
        max_values: usize,
    ) -> Result<serde_json::Value> {
        let job = self.job(owner, ticket)?;
        ensure!(job.readback, "only readback jobs contain CPU values");
        ensure!(
            job.state == JobState::Complete,
            "compute result is {}",
            job.state.name()
        );
        let value = job.layout.as_ref().unwrap().unpack(
            job.bytes.as_deref().context("missing readback data")?,
            max_values,
        )?;
        self.jobs.remove(&ticket);
        self.readbacks.remove(&ticket);
        Ok(value)
    }
    /// Queued work is removed. Submitted GPU work keeps running, but its result is suppressed.
    pub fn cancel(&mut self, owner: &Owner, ticket: Ticket) -> Result<()> {
        let state = self.job(owner, ticket)?.state.clone();
        if state == JobState::Queued {
            self.requests.retain(|request| {
                if request.command.ticket() == Some(ticket) {
                    self.stats.queued_upload_bytes -= request.command.upload_bytes();
                    false
                } else {
                    true
                }
            });
            self.readbacks.remove(&ticket);
        } else if state.terminal() && state != JobState::Cancelled {
            self.readbacks.remove(&ticket);
        }
        let job = self.jobs.get_mut(&ticket).unwrap();
        job.state = JobState::Cancelled;
        job.bytes = None;
        Ok(())
    }
    pub fn forget(&mut self, owner: &Owner, ticket: Ticket) -> Result<()> {
        let state = &self.job(owner, ticket)?.state;
        ensure!(
            state.terminal(),
            "cancel an unfinished compute job before forgetting it"
        );
        if *state != JobState::Cancelled {
            self.readbacks.remove(&ticket);
        }
        self.jobs.remove(&ticket);
        // A cancelled submitted readback still owns its slot until its callback arrives.
        Ok(())
    }
    pub fn release(&mut self, owner: &Owner, handle: Handle) -> Result<()> {
        ensure!(
            handle.world == self.world,
            "stale or cross-world compute handle"
        );
        let state = self
            .resources
            .get(&handle)
            .context("released compute handle")?;
        ensure!(!state.retiring, "released compute handle");
        ensure!(
            state.resource.scope == Scope::Scene || state.resource.owner == *owner,
            "compute handle belongs to another attachment"
        );
        let resource = state.resource.clone();
        // Reserve space for at most MAX_RESOURCES retirements even when the normal queue is full.
        self.enqueue(Command::Release(resource.clone()))?;
        self.resources.get_mut(&handle).unwrap().retiring = true;
        self.names.remove(&Self::name_key(
            &resource.owner,
            resource.scope,
            &resource.name,
        ));
        Ok(())
    }
    pub fn release_owner(&mut self, owner: &Owner) -> Result<()> {
        let tickets: Vec<_> = self
            .jobs
            .values()
            .filter(|job| job.owner == *owner)
            .map(|job| job.ticket)
            .collect();
        for ticket in tickets {
            self.cancel(owner, ticket)?;
            self.forget(owner, ticket)?;
        }
        let handles: Vec<_> = self
            .resources
            .values()
            .filter(|state| {
                !state.retiring
                    && state.resource.scope == Scope::Attachment
                    && state.resource.owner == *owner
            })
            .map(|state| state.resource.handle)
            .collect();
        for handle in handles {
            self.release(owner, handle)?;
        }
        Ok(())
    }
    /// This closure is the only queue-consumption boundary. On an abandoned encoder, return
    /// Deferred. Only return Submitted after queue.submit; presentation failure cannot replay it.
    pub fn submit_with(
        &mut self,
        submit: impl FnOnce(Batch<'_>, CompletionSink) -> Submission,
    ) -> Result<bool> {
        if self.requests.is_empty() || self.pending.len() >= MAX_SUBMISSIONS {
            return Ok(false);
        }
        let serial = self.next_serial()?;
        let (sink, _) = self.channel.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::sync_channel(MAX_SUBMISSIONS + MAX_READBACKS + 1);
            (CompletionSink(sender), receiver)
        });
        let outcome = submit(
            Batch {
                world: self.world,
                serial,
                requests: &self.requests,
            },
            sink.clone(),
        );
        if matches!(outcome, Submission::Deferred) {
            return Ok(false);
        }
        let mut pending = Pending {
            jobs: Vec::new(),
            releases: Vec::new(),
        };
        for request in self.requests.drain(..) {
            match &outcome {
                Submission::Submitted => {
                    match &request.command {
                        Command::Dispatch { .. } => self.stats.submitted_dispatches += 1,
                        Command::Readback { bytes, .. } => self.stats.readback_bytes += bytes,
                        _ => {}
                    }
                    self.stats.uploaded_bytes += request.command.upload_bytes() as u64;
                    if let Some(ticket) = request.command.ticket() {
                        if let Some(job) = self.jobs.get_mut(&ticket) {
                            job.state = JobState::Submitted;
                        }
                        if !matches!(request.command, Command::Readback { .. }) {
                            pending.jobs.push(ticket);
                        }
                    }
                    if let Command::Release(resource) = request.command {
                        pending.releases.push(resource.handle);
                    }
                }
                Submission::Rejected(error) => {
                    if let Some(ticket) = request.command.ticket() {
                        if let Some(job) = self.jobs.get_mut(&ticket) {
                            job.state = JobState::Failed(error.clone());
                        }
                        self.readbacks.remove(&ticket);
                    }
                    if let Command::Create(resource) = &request.command {
                        self.resources.get_mut(&resource.handle).unwrap().error =
                            Some(error.clone());
                    }
                    if let Command::Release(resource) = request.command {
                        pending.releases.push(resource.handle);
                    }
                }
                Submission::Deferred => unreachable!(),
            }
        }
        self.stats.queued_upload_bytes = 0;
        if matches!(outcome, Submission::Submitted) {
            self.pending.insert(serial, pending);
        } else {
            self.retire(pending);
        }
        Ok(true)
    }
    fn retire(&mut self, pending: Pending) {
        for ticket in pending.jobs {
            if let Some(job) = self.jobs.get_mut(&ticket)
                && job.state == JobState::Submitted
            {
                job.state = JobState::Complete;
            }
        }
        for handle in pending.releases {
            if let Some(state) = self.resources.remove(&handle) {
                self.stats.resource_bytes -= state.resource.kind.bytes();
            }
        }
    }
    /// Call exactly at a permitted simulation boundary, including a debugger single step.
    /// Paused repaints can poll the device without delivering any results into gameplay.
    pub fn begin_tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        while let Some(completion) = self
            .channel
            .as_ref()
            .and_then(|(_, receiver)| receiver.try_recv().ok())
        {
            match completion {
                Completion::Submission(serial) => {
                    if let Some(pending) = self.pending.remove(&serial) {
                        self.retire(pending);
                    }
                }
                Completion::Readback(ticket, result) => {
                    let Some(job) = self.jobs.get_mut(&ticket) else {
                        self.readbacks.remove(&ticket);
                        continue;
                    };
                    if job.state != JobState::Submitted {
                        self.readbacks.remove(&ticket);
                        continue;
                    }
                    match result {
                        Ok(bytes) if bytes.len() as u64 <= MAX_READBACK_BYTES => {
                            job.bytes = Some(bytes);
                            job.state = JobState::Complete;
                        }
                        Ok(_) => {
                            job.state =
                                JobState::Failed("executor exceeded readback byte budget".into());
                            self.readbacks.remove(&ticket);
                        }
                        Err(error) => {
                            job.state = JobState::Failed(error);
                            self.readbacks.remove(&ticket);
                        }
                    }
                }
                Completion::DeviceFailure(error) => {
                    for job in self.jobs.values_mut().filter(|job| !job.state.terminal()) {
                        job.state = JobState::Failed(error.clone());
                    }
                    for resource in self.resources.values_mut() {
                        resource.error = Some(error.clone());
                    }
                    self.requests.clear();
                    self.pending.clear();
                    self.readbacks.clear();
                    self.stats.queued_upload_bytes = 0;
                    self.capabilities.backend = None;
                }
            }
        }
    }
}
