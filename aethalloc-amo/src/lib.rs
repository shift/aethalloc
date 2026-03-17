//! AethAlloc AMO - Asynchronous Metadata Offloading
//!
//! This crate implements the SPSC ring buffer for offloading metadata
//! operations from the application core to the support core.

#![no_std]

pub mod command;
pub mod ring_buffer;
#[cfg(feature = "std")]
pub mod support_core;

pub use command::{RingCommand, RingEntry, RingPayload};
pub use ring_buffer::RingBuffer;
