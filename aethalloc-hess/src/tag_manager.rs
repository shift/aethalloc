//! Tag management for hardware-enforced spatial safety
//!
//! Provides a generic interface for memory tagging that abstracts over
//! ARM MTE and CHERI capability hardware.

use core::ptr::NonNull;

/// Tag value type (MTE uses 4-bit tags: 0-15)
pub type Tag = u16;

/// Maximum tag value for MTE (4 bits = 16 values, but 0 is reserved)
pub const MAX_TAG: Tag = 15;
pub const MIN_TAG: Tag = 1;

/// Result of tag operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagError {
    OutOfTags,
    InvalidTag,
    HardwareNotSupported,
    AlignmentError,
}

/// Tag allocation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagStrategy {
    Random,
    Sequential,
    RoundRobin,
}

/// Generic tag manager trait
pub trait TagManager {
    /// Allocate a new tag for a memory region
    fn allocate_tag(&mut self) -> Result<Tag, TagError>;

    /// Free a tag for reuse
    fn free_tag(&mut self, tag: Tag) -> Result<(), TagError>;

    /// Apply a tag to a pointer
    fn tag_pointer(&self, ptr: NonNull<u8>, tag: Tag) -> Result<NonNull<u8>, TagError>;

    /// Extract the tag from a pointer
    fn untag_pointer(&self, ptr: NonNull<u8>) -> NonNull<u8>;

    /// Get the tag from a tagged pointer
    fn get_tag(&self, ptr: NonNull<u8>) -> Tag;

    /// Store the allocation tag to memory
    fn store_tag(&self, ptr: NonNull<u8>, tag: Tag) -> Result<(), TagError>;

    /// Load the allocation tag from memory
    fn load_tag(&self, ptr: NonNull<u8>) -> Tag;
}

/// Software fallback tag manager (for testing on non-MTE hardware)
#[derive(Debug, Clone)]
pub struct SoftwareTagManager {
    next_tag: Tag,
    used_tags: u16,
}

impl SoftwareTagManager {
    pub const fn new() -> Self {
        Self {
            next_tag: MIN_TAG,
            used_tags: 0,
        }
    }

    fn is_tag_used(&self, tag: Tag) -> bool {
        (self.used_tags & (1 << tag)) != 0
    }

    fn mark_tag_used(&mut self, tag: Tag) {
        self.used_tags |= 1 << tag;
    }

    fn mark_tag_free(&mut self, tag: Tag) {
        self.used_tags &= !(1 << tag);
    }
}

impl Default for SoftwareTagManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TagManager for SoftwareTagManager {
    fn allocate_tag(&mut self) -> Result<Tag, TagError> {
        for _ in MIN_TAG..=MAX_TAG {
            let tag = self.next_tag;
            self.next_tag = if self.next_tag >= MAX_TAG {
                MIN_TAG
            } else {
                self.next_tag + 1
            };

            if !self.is_tag_used(tag) {
                self.mark_tag_used(tag);
                return Ok(tag);
            }
        }
        Err(TagError::OutOfTags)
    }

    fn free_tag(&mut self, tag: Tag) -> Result<(), TagError> {
        if !(MIN_TAG..=MAX_TAG).contains(&tag) {
            return Err(TagError::InvalidTag);
        }
        self.mark_tag_free(tag);
        Ok(())
    }

    fn tag_pointer(&self, ptr: NonNull<u8>, tag: Tag) -> Result<NonNull<u8>, TagError> {
        if tag > MAX_TAG {
            return Err(TagError::InvalidTag);
        }
        let addr = ptr.as_ptr() as usize;
        let tagged_addr = (addr & !0x0F00000000000000) | ((tag as usize) << 56);
        Ok(unsafe { NonNull::new_unchecked(tagged_addr as *mut u8) })
    }

    fn untag_pointer(&self, ptr: NonNull<u8>) -> NonNull<u8> {
        let addr = ptr.as_ptr() as usize;
        let untagged_addr = addr & !0x0F00000000000000;
        unsafe { NonNull::new_unchecked(untagged_addr as *mut u8) }
    }

    fn get_tag(&self, ptr: NonNull<u8>) -> Tag {
        let addr = ptr.as_ptr() as usize;
        ((addr >> 56) & 0xF) as Tag
    }

    fn store_tag(&self, _ptr: NonNull<u8>, _tag: Tag) -> Result<(), TagError> {
        Ok(())
    }

    fn load_tag(&self, _ptr: NonNull<u8>) -> Tag {
        0
    }
}

/// Tagged allocation metadata
#[derive(Debug, Clone, Copy)]
pub struct TaggedAllocation {
    pub ptr: NonNull<u8>,
    pub size: usize,
    pub tag: Tag,
}

impl TaggedAllocation {
    pub fn new(ptr: NonNull<u8>, size: usize, tag: Tag) -> Self {
        Self { ptr, size, tag }
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.size) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_software_tag_manager_allocate() {
        let mut manager = SoftwareTagManager::new();
        let tag = manager.allocate_tag().unwrap();
        assert!(tag >= MIN_TAG && tag <= MAX_TAG);
    }

    #[test]
    fn test_software_tag_manager_free_reuse() {
        let mut manager = SoftwareTagManager::new();
        let _tag1 = manager.allocate_tag().unwrap();
        let tag2 = manager.allocate_tag().unwrap();
        manager.free_tag(tag2).unwrap();
        let _tag3 = manager.allocate_tag().unwrap();
    }

    #[test]
    fn test_software_tag_manager_tag_pointer() {
        let manager = SoftwareTagManager::new();
        let ptr = NonNull::new(0x1000 as *mut u8).unwrap();
        let tagged = manager.tag_pointer(ptr, 5).unwrap();
        let extracted_tag = manager.get_tag(tagged);
        assert_eq!(extracted_tag, 5);
    }

    #[test]
    fn test_software_tag_manager_untag_pointer() {
        let manager = SoftwareTagManager::new();
        let ptr = NonNull::new(0x1000 as *mut u8).unwrap();
        let tagged = manager.tag_pointer(ptr, 7).unwrap();
        let untagged = manager.untag_pointer(tagged);
        assert_eq!(untagged, ptr);
    }

    #[test]
    fn test_tagged_allocation() {
        let mut buffer = [0u8; 64];
        let ptr = NonNull::new(buffer.as_mut_ptr()).unwrap();
        let alloc = TaggedAllocation::new(ptr, 64, 3);
        assert_eq!(alloc.size, 64);
        assert_eq!(alloc.tag, 3);
    }

    #[test]
    fn test_tag_out_of_range() {
        let manager = SoftwareTagManager::new();
        let ptr = NonNull::new(0x1000 as *mut u8).unwrap();
        assert!(manager.tag_pointer(ptr, 20).is_err());
    }
}
