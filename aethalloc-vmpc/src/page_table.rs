//! Page table entry management via /proc/self/pagemap
//!
//! Provides utilities to query page table information from the kernel.

pub const PAGE_SIZE: usize = 4096;
pub const PAGEMAP_ENTRY_SIZE: usize = 8;

/// Page table entry from /proc/self/pagemap
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageMapEntry {
    pub raw: u64,
}

impl PageMapEntry {
    pub const fn new(raw: u64) -> Self {
        Self { raw }
    }

    pub fn is_present(&self) -> bool {
        (self.raw >> 63) & 1 == 1
    }

    pub fn is_swapped(&self) -> bool {
        (self.raw >> 62) & 1 == 1
    }

    pub fn is_file_mapped(&self) -> bool {
        (self.raw >> 61) & 1 == 1
    }

    pub fn page_frame_number(&self) -> u64 {
        self.raw & ((1u64 << 55) - 1)
    }

    pub fn physical_address(&self) -> Option<u64> {
        if self.is_present() && !self.is_swapped() {
            Some(self.page_frame_number() * PAGE_SIZE as u64)
        } else {
            None
        }
    }
}

/// Page utilization info for a memory region
#[derive(Clone, Copy, Debug, Default)]
pub struct PageUtilization {
    pub page_addr: usize,
    pub allocated_bytes: usize,
    pub total_bytes: usize,
}

impl PageUtilization {
    pub fn utilization(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.allocated_bytes as f32 / self.total_bytes as f32
        }
    }

    pub fn is_sparse(&self, threshold: f32) -> bool {
        self.utilization() < threshold
    }
}

/// Tracker for page table information
pub struct PageTableTracker {
    pub pagemap_fd: Option<i32>,
}

impl PageTableTracker {
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        {
            let fd = unsafe { open_pagemap() };
            Self {
                pagemap_fd: Some(fd),
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Self { pagemap_fd: None }
        }
    }

    /// Query page map entry for a virtual address
    pub fn query_page(&self, addr: usize) -> Option<PageMapEntry> {
        #[cfg(feature = "std")]
        {
            self.pagemap_fd
                .and_then(|fd| unsafe { read_pagemap_entry(fd, addr).ok() })
        }
        #[cfg(not(feature = "std"))]
        {
            let _ = addr;
            None
        }
    }

    /// Check if two addresses map to the same physical page
    pub fn same_physical_page(&self, addr1: usize, addr2: usize) -> bool {
        match (self.query_page(addr1), self.query_page(addr2)) {
            (Some(e1), Some(e2)) => {
                e1.is_present()
                    && e2.is_present()
                    && e1.page_frame_number() == e2.page_frame_number()
            }
            _ => false,
        }
    }

    /// Get physical address for a virtual address
    pub fn physical_address(&self, addr: usize) -> Option<u64> {
        self.query_page(addr).and_then(|e| e.physical_address())
    }
}

impl Default for PageTableTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PageTableTracker {
    fn drop(&mut self) {
        #[cfg(feature = "std")]
        if let Some(fd) = self.pagemap_fd {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

#[cfg(feature = "std")]
unsafe fn open_pagemap() -> i32 {
    let path = b"/proc/self/pagemap\0";
    let fd = libc::open(path.as_ptr() as *const i8, libc::O_RDONLY | libc::O_CLOEXEC);
    if fd < 0 {
        panic!("Failed to open /proc/self/pagemap");
    }
    fd
}

#[cfg(feature = "std")]
unsafe fn read_pagemap_entry(fd: i32, addr: usize) -> Result<PageMapEntry, i32> {
    let page_offset = (addr / PAGE_SIZE) as i64;
    let file_offset = page_offset * PAGEMAP_ENTRY_SIZE as i64;

    let mut entry: u64 = 0;
    let n = libc::pread(
        fd,
        &mut entry as *mut u64 as *mut core::ffi::c_void,
        core::mem::size_of::<u64>(),
        file_offset,
    );

    if n != core::mem::size_of::<u64>() as isize {
        return Err(-1);
    }

    Ok(PageMapEntry::new(entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagemap_entry_present() {
        let entry = PageMapEntry::new(1u64 << 63);
        assert!(entry.is_present());
        assert!(!entry.is_swapped());
    }

    #[test]
    fn test_pagemap_entry_swapped() {
        let entry = PageMapEntry::new(1u64 << 62);
        assert!(entry.is_swapped());
        assert!(!entry.is_present());
    }

    #[test]
    fn test_pagemap_entry_pfn() {
        let entry = PageMapEntry::new(0x1234);
        assert_eq!(entry.page_frame_number(), 0x1234);
    }

    #[test]
    fn test_page_utilization() {
        let util = PageUtilization {
            page_addr: 0x1000,
            allocated_bytes: 1024,
            total_bytes: 4096,
        };
        assert!((util.utilization() - 0.25).abs() < 0.01);
        assert!(util.is_sparse(0.5));
        assert!(!util.is_sparse(0.1));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_query_stack_page() {
        let tracker = PageTableTracker::new();
        let stack_var: usize = 42;
        let entry = tracker.query_page(&stack_var as *const usize as usize);
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.is_present());
    }
}
