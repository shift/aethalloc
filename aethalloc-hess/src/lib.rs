//! AethAlloc HESS - Hardware-Enforced Spatial Safety
//!
//! This crate provides memory tagging and capability-based safety:
//! - ARM MTE (Memory Tagging Extension) support
//! - CHERI capability bounds checking
//! - Software fallback for non-hardware platforms

#![no_std]

#[cfg(feature = "aethalloc-mte")]
pub mod mte;

#[cfg(feature = "aethalloc-cheri")]
pub mod cheri;

pub mod tag_manager;

pub use tag_manager::{
    SoftwareTagManager, Tag, TagError, TagManager, TagStrategy, TaggedAllocation, MAX_TAG, MIN_TAG,
};

#[cfg(feature = "aethalloc-mte")]
pub use mte::MteTagManager;

#[cfg(feature = "aethalloc-cheri")]
pub use cheri::{CapabilityMeta, CheriTagManager};
