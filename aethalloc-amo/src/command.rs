//! Commands sent from application core to support core via ring buffer

/// Commands for the support core
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RingCommand {
    /// Return block to global pool
    FreeBlock = 0,
    /// Trigger VMPC on a memory region
    CompactionRequest = 2,
    /// Update MTE tag
    TagUpdate = 4,
    /// Telemetry from application thread
    StatsReport = 3,
    /// No operation (padding/empty)
    #[default]
    NoOp = 255,
}

/// Payload for FreeBlock command
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FreeBlockPayload {
    pub ptr: *mut u8,
    pub size: usize,
    pub size_class: u8,
}

/// Payload for CompactionRequest command
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CompactionRequestPayload {
    pub start_addr: *mut u8,
    pub length: usize,
}

/// Payload for TagUpdate command
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TagUpdatePayload {
    pub ptr: *mut u8,
    pub old_tag: u16,
    pub new_tag: u16,
}

/// Payload for StatsReport command
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsReportPayload {
    pub thread_id: u64,
    pub allocs: u64,
    pub frees: u64,
}

/// 64-byte ring buffer entry
/// Layout: [command: u8][reserved: 7 bytes][payload: 56 bytes]
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct RingEntry {
    /// Command type
    pub command: RingCommand,
    /// Reserved for alignment
    _reserved: [u8; 7],
    /// Payload union (56 bytes)
    pub payload: RingPayload,
}

impl RingEntry {
    /// Create a new entry with the given command and payload
    pub fn new(command: RingCommand, payload: RingPayload) -> Self {
        Self {
            command,
            _reserved: [0; 7],
            payload,
        }
    }
}

impl Default for RingEntry {
    fn default() -> Self {
        Self {
            command: RingCommand::NoOp,
            _reserved: [0; 7],
            payload: RingPayload::default(),
        }
    }
}

impl core::fmt::Debug for RingEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RingEntry")
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

/// Union of all payload types (56 bytes to fit in 64-byte entry with header)
#[repr(C)]
pub union RingPayload {
    pub free_block: FreeBlockPayload,
    pub compaction: CompactionRequestPayload,
    pub tag_update: TagUpdatePayload,
    pub stats: StatsReportPayload,
    pub raw: [u8; 56],
}

impl Default for RingPayload {
    fn default() -> Self {
        Self { raw: [0; 56] }
    }
}

impl Clone for RingPayload {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for RingPayload {}

const _: () = assert!(core::mem::size_of::<RingEntry>() == 64);
const _: () = assert!(core::mem::align_of::<RingEntry>() == 64);
const _: () = assert!(core::mem::size_of::<RingPayload>() == 56);
