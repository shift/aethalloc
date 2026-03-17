//! AethAlloc VMPC - Virtual Memory Page Compaction
//!
//! This crate implements page compaction for memory defragmentation:
//! - Page table tracking via /proc/self/pagemap
//! - mremap-based page migration
//! - process_vm_writev for cross-process memory operations

#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod compactor;
pub mod page_table;

pub use compactor::{CompactConfig, CompactResult, Compactor};
pub use page_table::{PageMapEntry, PageTableTracker, PageUtilization};
