//! Validated compute authoring and commands. No GPU, ECS, window, or importer dependency.
mod layout;
mod resource;
mod runtime;
mod shader;
pub use layout::{Layout, Scalar, Shape};
pub use resource::{Capabilities, Handle, Owner, Resource, ResourceKind, Scope};
pub use runtime::{
    Batch, Command, CompletionSink, Dispatch, Job, JobState, Request, Runtime, Statistics,
    Submission, Ticket,
};
pub use runtime::{
    MAX_COMMANDS, MAX_JOBS, MAX_READBACK_BYTES, MAX_READBACKS, MAX_RESOURCE_BYTES, MAX_RESOURCES,
    MAX_SUBMISSIONS, MAX_UPLOAD_BYTES,
};
pub use shader::{Binding, BindingKind, EntryPoint, Kernel, TextureFormat};

/// Authoring limits bound CPU parsing and packing independently of device limits.
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_BINDINGS: usize = 16;
