//! AethAlloc Core - no_std allocator logic
//!
//! This crate contains the core allocation algorithms:
//! - Size class classification
//! - Page management (mmap/munmap wrappers)
//! - Slab allocator for small allocations
//! - Buddy allocator for medium allocations
//! - Thread-local caches

#![no_std]

extern crate libc;

#[cfg(test)]
extern crate std;

pub mod buddy;
pub mod global_pool;
pub mod hess;
pub mod magazine;
pub mod page;
pub mod size_class;
pub mod slab;
pub mod thread_local;
pub mod vmpc;

pub use global_pool::GlobalPools;
pub use hess::{tag_allocation, verify_tag, Tag, TaggedAllocation, MAX_TAG, MIN_TAG};
pub use magazine::{
    GlobalMagazinePools, Magazine, MagazineNode, MetadataAllocator, MAGAZINE_CAPACITY,
    NUM_SIZE_CLASSES,
};
pub use vmpc::try_compact_region;
