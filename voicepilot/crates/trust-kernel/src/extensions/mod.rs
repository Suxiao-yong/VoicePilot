//! Runtime extension descriptors and immutable per-task catalog snapshots.

pub mod registry;
pub mod types;

pub use registry::ExtensionCatalog;
pub use types::{ExecutionTarget, ExtensionDescriptor, ExtensionSnapshot, ExtensionSource};
