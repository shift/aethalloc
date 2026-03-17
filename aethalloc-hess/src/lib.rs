//! AethAlloc HESS - Hardware-Enforced Spatial Safety
//!
//! This crate provides memory tagging and capability-based safety:
//! - ARM MTE (Memory Tagging Extension) support
//! - CHERI capability bounds checking
//! - x86_64 LAM/UAI pointer masking
//! - Software fallback for non-hardware platforms

#![no_std]

#[cfg(feature = "aethalloc-mte")]
pub mod mte;

#[cfg(feature = "aethalloc-cheri")]
pub mod cheri;

pub mod tag_manager;
pub mod x86;

#[cfg(feature = "aethalloc-mte")]
pub use mte::MteTagManager;

#[cfg(feature = "aethalloc-cheri")]
pub use cheri::{CapabilityMeta, CheriTagManager};

pub use tag_manager::{
    SoftwareTagManager, Tag, TagError, TagManager, TagStrategy, TaggedAllocation, MAX_TAG, MIN_TAG,
};
pub use x86::X86TagManager;
