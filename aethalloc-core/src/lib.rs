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
pub mod page;
pub mod size_class;
pub mod slab;
pub mod thread_local;
