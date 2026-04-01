//! Support core that consumes commands from the ring buffer
//!
//! This module implements the support core thread that asynchronously
//! processes metadata operations offloaded from the application core.
//!
//! Optimizations:
//! - Adaptive backoff: spin -> yield -> park to minimize CPU waste
//! - Batch processing: drain multiple entries per wake cycle

use crate::command::{RingCommand, RingEntry};
use crate::ring_buffer::RingBuffer;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
use std::thread;
#[cfg(feature = "std")]
use std::time::Duration;

/// Statistics accumulated by the support core
pub struct SupportCoreStats {
    pub blocks_freed: u64,
    pub compactions_run: u64,
    pub tags_updated: u64,
    pub stats_reports_received: u64,
    pub total_allocs_seen: u64,
    pub total_frees_seen: u64,
    pub idle_parks: u64,
}

impl Default for SupportCoreStats {
    fn default() -> Self {
        Self {
            blocks_freed: 0,
            compactions_run: 0,
            tags_updated: 0,
            stats_reports_received: 0,
            total_allocs_seen: 0,
            total_frees_seen: 0,
            idle_parks: 0,
        }
    }
}

/// Support core that processes ring buffer commands
pub struct SupportCore<const N: usize> {
    ring_buffer: &'static RingBuffer<N>,
    running: bool,
    stats: SupportCoreStats,
    idle_count: u32,
}

impl<const N: usize> SupportCore<N> {
    pub fn new(ring_buffer: &'static RingBuffer<N>) -> Self {
        Self {
            ring_buffer,
            running: true,
            stats: SupportCoreStats::default(),
            idle_count: 0,
        }
    }

    pub fn run(&mut self) {
        const MAX_SPINS: u32 = 64;
        const PARK_DURATION: Duration = Duration::from_micros(100);

        while self.running {
            if let Some(entry) = self.ring_buffer.try_pop() {
                self.idle_count = 0;
                self.handle_command(entry);
            } else {
                self.idle_count += 1;

                if self.idle_count < 16 {
                    core::hint::spin_loop();
                } else if self.idle_count < MAX_SPINS {
                    #[cfg(feature = "std")]
                    thread::yield_now();
                } else {
                    #[cfg(feature = "std")]
                    {
                        self.stats.idle_parks += 1;
                        thread::sleep(PARK_DURATION);
                    }
                    #[cfg(not(feature = "std"))]
                    {
                        self.idle_count = MAX_SPINS / 2;
                    }
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn stats(&self) -> &SupportCoreStats {
        &self.stats
    }

    pub fn handle_command(&mut self, entry: RingEntry) {
        match entry.command {
            RingCommand::FreeBlock => {
                let payload = unsafe { entry.payload.free_block };
                if !payload.ptr.is_null() {
                    unsafe {
                        libc::free(payload.ptr as *mut libc::c_void);
                    }
                    self.stats.blocks_freed += 1;
                }
            }
            RingCommand::CompactionRequest => {
                let payload = unsafe { entry.payload.compaction };
                if !payload.start_addr.is_null() && payload.length > 0 {
                    #[cfg(all(feature = "std", feature = "vmpc"))]
                    unsafe {
                        use aethalloc_vmpc::compactor::{CompactConfig, Compactor};
                        let compactor = Compactor::new(CompactConfig::default());
                        let ptr = core::ptr::NonNull::new(payload.start_addr);
                        if let Some(nn) = ptr {
                            let _ = compactor.compact_pages(nn, payload.length);
                        }
                    }
                    self.stats.compactions_run += 1;
                }
            }
            RingCommand::TagUpdate => {
                let payload = unsafe { entry.payload.tag_update };
                if !payload.ptr.is_null() {
                    #[cfg(feature = "std")]
                    {
                        use aethalloc_hess::tag_manager::{SoftwareTagManager, TagManager};
                        let mgr = SoftwareTagManager::new();
                        let ptr = core::ptr::NonNull::new(payload.ptr);
                        if let Some(nn) = ptr {
                            let _ = mgr.store_tag(nn, payload.new_tag);
                        }
                    }
                    self.stats.tags_updated += 1;
                }
            }
            RingCommand::StatsReport => {
                let payload = unsafe { entry.payload.stats };
                self.stats.stats_reports_received += 1;
                self.stats.total_allocs_seen += payload.allocs;
                self.stats.total_frees_seen += payload.frees;
            }
            RingCommand::NoOp => {}
        }
    }
}

/// Spawn the support core worker thread
///
/// # Safety
/// The ring buffer must have static lifetime and not be dropped
/// while the support core thread is running.
#[cfg(feature = "std")]
pub unsafe fn spawn_support_core<const N: usize>(
    ring_buffer: &'static RingBuffer<N>,
) -> std::thread::JoinHandle<()> {
    use std::string::ToString;
    std::thread::Builder::new()
        .name("aethalloc-support-core".to_string())
        .spawn(move || {
            let mut core_worker = SupportCore::new(ring_buffer);
            core_worker.run();
        })
        .expect("failed to spawn support core thread")
}
