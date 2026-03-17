#![no_std]

#[cfg(feature = "aethalloc-mte")]
pub mod mte;

#[cfg(feature = "aethalloc-cheri")]
pub mod cheri;

pub mod tag_manager;
