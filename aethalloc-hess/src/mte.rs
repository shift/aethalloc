//! ARM Memory Tagging Extension (MTE) support
//!
//! Provides hardware-accelerated memory tagging on ARMv8.5+ processors.
//! MTE uses 4-bit tags stored in the top bits of pointers.

#![cfg(feature = "aethalloc-mte")]

use core::arch::asm;
use core::ptr::NonNull;

use crate::tag_manager::{Tag, TagError, TagManager, MAX_TAG, MIN_TAG};

/// ARM MTE tag manager
#[derive(Debug, Clone)]
pub struct MteTagManager {
    used_tags: u16,
    next_tag: Tag,
}

impl MteTagManager {
    pub const fn new() -> Self {
        Self {
            used_tags: 0,
            next_tag: MIN_TAG,
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

impl Default for MteTagManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TagManager for MteTagManager {
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
        if tag < MIN_TAG || tag > MAX_TAG {
            return Err(TagError::InvalidTag);
        }
        self.mark_tag_free(tag);
        Ok(())
    }

    fn tag_pointer(&self, ptr: NonNull<u8>, tag: Tag) -> Result<NonNull<u8>, TagError> {
        if tag > MAX_TAG {
            return Err(TagError::InvalidTag);
        }
        let tagged = unsafe { addg(ptr.as_ptr(), tag) };
        Ok(unsafe { NonNull::new_unchecked(tagged) })
    }

    fn untag_pointer(&self, ptr: NonNull<u8>) -> NonNull<u8> {
        let untagged = unsafe { subp(ptr.as_ptr(), ptr.as_ptr()) };
        unsafe { NonNull::new_unchecked(untagged) }
    }

    fn get_tag(&self, ptr: NonNull<u8>) -> Tag {
        unsafe { ldg(ptr.as_ptr()) as Tag }
    }

    fn store_tag(&self, ptr: NonNull<u8>, tag: Tag) -> Result<(), TagError> {
        if tag > MAX_TAG {
            return Err(TagError::InvalidTag);
        }
        let tagged = unsafe { addg(ptr.as_ptr(), tag) };
        unsafe { stg(tagged) };
        Ok(())
    }

    fn load_tag(&self, ptr: NonNull<u8>) -> Tag {
        unsafe { ldg(ptr.as_ptr()) as Tag }
    }
}

/// Insert Random Tag (IRG)
///
/// Generates a random tag and inserts it into the pointer.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn irg(ptr: *mut u8, mask: u64) -> *mut u8 {
    let result: *mut u8;
    asm!(
        "irg {0}, {1}, {2}",
        out(reg) result,
        in(reg) ptr,
        in(reg) mask,
        options(nostack, pure, readonly)
    );
    result
}

/// Add Tag (ADDG)
///
/// Adds a tag offset to the pointer's tag field.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn addg(ptr: *mut u8, tag_offset: u16) -> *mut u8 {
    let result: *mut u8;
    asm!(
        "addg {0}, {1}, #0, {2}",
        out(reg) result,
        in(reg) ptr,
        in(reg) tag_offset as u64,
        options(nostack, pure, readonly)
    );
    result
}

/// Subtract Pointer (SUBP)
///
/// Subtracts two pointers, removing the tag from the result.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn subp(ptr1: *mut u8, ptr2: *mut u8) -> *mut u8 {
    let result: *mut u8;
    asm!(
        "subp {0}, {1}, {2}",
        out(reg) result,
        in(reg) ptr1,
        in(reg) ptr2,
        options(nostack, pure, readonly)
    );
    result
}

/// Store Allocation Tag (STG)
///
/// Stores the pointer's tag to the allocation tag memory.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
/// ptr must be 16-byte aligned and point to valid tagged memory.
#[inline(always)]
pub unsafe fn stg(ptr: *mut u8) {
    asm!(
        "stg {0}, [{0}]",
        in(reg) ptr,
        options(nostack)
    );
}

/// Load Allocation Tag (LDG)
///
/// Loads the allocation tag from memory and returns it.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn ldg(ptr: *mut u8) -> u64 {
    let result: *mut u8;
    asm!(
        "ldg {0}, [{1}]",
        out(reg) result,
        in(reg) ptr,
        options(nostack, pure, readonly)
    );
    result as u64
}

/// Store Allocation Tag Pair (STZG)
///
/// Stores the allocation tag and zeros the memory.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn stzg(ptr: *mut u8) {
    asm!(
        "stzg {0}, [{0}]",
        in(reg) ptr,
        options(nostack)
    );
}

/// Get Mask Inserted (GMI)
///
/// Returns a mask with the tag bit set.
///
/// # Safety
/// Requires ARMv8.5-A MTE support.
#[inline(always)]
pub unsafe fn gmi(ptr: *mut u8, mask: u64) -> u64 {
    let result: u64;
    asm!(
        "gmi {0}, {1}, {2}",
        out(reg) result,
        in(reg) ptr,
        in(reg) mask,
        options(nostack, pure, readonly)
    );
    result
}

/// Check if MTE is supported on this CPU
pub fn is_mte_supported() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        let mut id_aa64pfr1: u64;
        unsafe {
            asm!(
                "mrs {0}, id_aa64pfr1_el1",
                out(reg) id_aa64pfr1,
                options(nostack, pure, readonly)
            );
        }
        let mte = (id_aa64pfr1 >> 8) & 0xF;
        mte >= 1
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mte_tag_manager_creation() {
        let manager = MteTagManager::new();
        assert_eq!(manager.used_tags, 0);
    }

    #[test]
    fn test_mte_tag_allocate() {
        let mut manager = MteTagManager::new();
        let tag = manager.allocate_tag().unwrap();
        assert!(tag >= MIN_TAG && tag <= MAX_TAG);
    }

    #[test]
    fn test_mte_supported_check() {
        let _ = is_mte_supported();
    }
}
