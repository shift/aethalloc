//! Support core that consumes commands from the ring buffer
//!
//! This module implements the support core thread that asynchronously
//! processes metadata operations offloaded from the application core.

use crate::command::{RingCommand, RingEntry};
use crate::ring_buffer::RingBuffer;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
use std::thread;

/// Support core that processes ring buffer commands
pub struct SupportCore<const N: usize> {
    ring_buffer: &'static RingBuffer<N>,
    running: bool,
}

impl<const N: usize> SupportCore<N> {
    pub fn new(ring_buffer: &'static RingBuffer<N>) -> Self {
        Self {
            ring_buffer,
            running: true,
        }
    }

    pub fn run(&mut self) {
        while self.running {
            if let Some(entry) = self.ring_buffer.try_pop() {
                self.handle_command(entry);
            } else {
                #[cfg(feature = "std")]
                thread::yield_now();
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn handle_command(&mut self, entry: RingEntry) {
        match entry.command {
            RingCommand::FreeBlock => {
                let payload = unsafe { entry.payload.free_block };
                // SAFETY: payload.ptr was allocated with payload.size bytes
                let _ = payload.ptr;
                let _ = payload.size_class;
                let _ = payload.size;
            }
            RingCommand::CompactionRequest => {
                let payload = unsafe { entry.payload.compaction };
                let _ = payload.start_addr;
                let _ = payload.length;
            }
            RingCommand::TagUpdate => {
                let payload = unsafe { entry.payload.tag_update };
                let _ = payload.ptr;
                let _ = payload.old_tag;
                let _ = payload.new_tag;
            }
            RingCommand::StatsReport => {
                let payload = unsafe { entry.payload.stats };
                let _ = payload.thread_id;
                let _ = payload.allocs;
                let _ = payload.frees;
            }
            RingCommand::NoOp => {}
        }
    }
}
