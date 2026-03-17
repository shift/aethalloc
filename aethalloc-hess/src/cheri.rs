//! CHERI capability hardware support
//!
//! Provides capability-based memory safety on CHERI-enabled processors.
//! CHERI uses fat pointers with embedded bounds and permissions.

#![cfg(feature = "aethalloc-cheri")]

use core::ptr::NonNull;

use crate::tag_manager::{Tag, TagError, TagManager, MAX_TAG};

/// CHERI capability metadata
#[derive(Debug, Clone, Copy)]
pub struct CapabilityMeta {
    pub base: usize,
    pub length: usize,
    pub offset: usize,
    pub permissions: u32,
    pub tag: bool,
}

/// CHERI capability permissions
pub mod permissions {
    pub const LOAD: u32 = 1 << 0;
    pub const STORE: u32 = 1 << 1;
    pub const EXECUTE: u32 = 1 << 2;
    pub const LOAD_CAP: u32 = 1 << 3;
    pub const STORE_CAP: u32 = 1 << 4;
    pub const LOAD_MUTABLE: u32 = 1 << 5;
    pub const STORE_LOCAL: u32 = 1 << 6;
    pub const SEAL: u32 = 1 << 7;
    pub const ALL: u32 = 0xFF;
    pub const READ_WRITE: u32 = LOAD | STORE | LOAD_CAP | STORE_CAP;
}

/// CHERI tag manager
#[derive(Debug, Clone, Default)]
pub struct CheriTagManager {
    used_tags: u16,
}

impl CheriTagManager {
    pub const fn new() -> Self {
        Self { used_tags: 0 }
    }
}

impl TagManager for CheriTagManager {
    fn allocate_tag(&mut self) -> Result<Tag, TagError> {
        for tag in 1..=MAX_TAG {
            if (self.used_tags & (1 << tag)) == 0 {
                self.used_tags |= 1 << tag;
                return Ok(tag);
            }
        }
        Err(TagError::OutOfTags)
    }

    fn free_tag(&mut self, tag: Tag) -> Result<(), TagError> {
        if tag == 0 || tag > MAX_TAG {
            return Err(TagError::InvalidTag);
        }
        self.used_tags &= !(1 << tag);
        Ok(())
    }

    fn tag_pointer(&self, ptr: NonNull<u8>, _tag: Tag) -> Result<NonNull<u8>, TagError> {
        Ok(ptr)
    }

    fn untag_pointer(&self, ptr: NonNull<u8>) -> NonNull<u8> {
        ptr
    }

    fn get_tag(&self, _ptr: NonNull<u8>) -> Tag {
        0
    }

    fn store_tag(&self, _ptr: NonNull<u8>, _tag: Tag) -> Result<(), TagError> {
        Ok(())
    }

    fn load_tag(&self, _ptr: NonNull<u8>) -> Tag {
        0
    }
}

/// Get capability length (bounds)
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_get_length(cap: *const u8) -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        let len: usize;
        core::arch::asm!(
            "cgetlen {0}, {1}",
            out(reg) len,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        len
    }
    #[cfg(target_arch = "aarch64")]
    {
        let len: usize;
        core::arch::asm!(
            "cgetlen {0}, {1}",
            out(reg) len,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        len
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = cap;
        0
    }
}

/// Get capability base address
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_get_base(cap: *const u8) -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        let base: usize;
        core::arch::asm!(
            "cgetbase {0}, {1}",
            out(reg) base,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        base
    }
    #[cfg(target_arch = "aarch64")]
    {
        let base: usize;
        core::arch::asm!(
            "cgetbase {0}, {1}",
            out(reg) base,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        base
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = cap;
        0
    }
}

/// Get capability offset
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_get_offset(cap: *const u8) -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        let offset: usize;
        core::arch::asm!(
            "cgetoffset {0}, {1}",
            out(reg) offset,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        offset
    }
    #[cfg(target_arch = "aarch64")]
    {
        let offset: usize;
        core::arch::asm!(
            "cgetoffset {0}, {1}",
            out(reg) offset,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        offset
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = cap;
        0
    }
}

/// Check if capability has a valid tag
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_get_tag(cap: *const u8) -> bool {
    #[cfg(target_arch = "riscv64")]
    {
        let tag: usize;
        core::arch::asm!(
            "cgettag {0}, {1}",
            out(reg) tag,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        tag != 0
    }
    #[cfg(target_arch = "aarch64")]
    {
        let tag: usize;
        core::arch::asm!(
            "cgettag {0}, {1}",
            out(reg) tag,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        tag != 0
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = cap;
        false
    }
}

/// Get capability permissions
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_get_perms(cap: *const u8) -> u32 {
    #[cfg(target_arch = "riscv64")]
    {
        let perms: usize;
        core::arch::asm!(
            "cgetperm {0}, {1}",
            out(reg) perms,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        perms as u32
    }
    #[cfg(target_arch = "aarch64")]
    {
        let perms: usize;
        core::arch::asm!(
            "cgetperm {0}, {1}",
            out(reg) perms,
            in(reg) cap,
            options(nostack, pure, readonly)
        );
        perms as u32
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = cap;
        0
    }
}

/// Set capability bounds
///
/// # Safety
/// Requires CHERI hardware support.
#[inline(always)]
pub unsafe fn cheri_cap_bounds_set(cap: *mut u8, base: usize, length: usize) -> *mut u8 {
    #[cfg(target_arch = "riscv64")]
    {
        let result: *mut u8;
        core::arch::asm!(
            "csetboundsexact {0}, {1}, {2}",
            "csetaddr {0}, {0}, {3}",
            out(reg) result,
            in(reg) cap,
            in(reg) length,
            in(reg) base,
            options(nostack)
        );
        result
    }
    #[cfg(target_arch = "aarch64")]
    {
        let result: *mut u8;
        core::arch::asm!(
            "csetbound {0}, {1}, {2}",
            out(reg) result,
            in(reg) cap,
            in(reg) length,
            options(nostack)
        );
        result
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        let _ = (base, length);
        cap
    }
}

/// Get full capability metadata
///
/// # Safety
/// Requires CHERI hardware support.
pub unsafe fn get_capability_meta(cap: *const u8) -> CapabilityMeta {
    CapabilityMeta {
        base: cheri_cap_get_base(cap),
        length: cheri_cap_get_length(cap),
        offset: cheri_cap_get_offset(cap),
        permissions: cheri_cap_get_perms(cap),
        tag: cheri_cap_get_tag(cap),
    }
}

/// Create a bounded capability
///
/// # Safety
/// - base must point to valid memory
/// - length must not exceed the actual allocation
pub unsafe fn create_bounded_capability(
    base: NonNull<u8>,
    length: usize,
    perms: u32,
) -> Option<NonNull<u8>> {
    let cap = cheri_cap_bounds_set(base.as_ptr(), 0, length);

    let actual_perms = cheri_cap_get_perms(cap);
    if (actual_perms & perms) != perms {
        return None;
    }

    if !cheri_cap_get_tag(cap) {
        return None;
    }

    Some(NonNull::new_unchecked(cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cheri_tag_manager_creation() {
        let manager = CheriTagManager::new();
        assert_eq!(manager.used_tags, 0);
    }

    #[test]
    fn test_cheri_tag_allocate() {
        let mut manager = CheriTagManager::new();
        let tag = manager.allocate_tag().unwrap();
        assert!(tag >= 1 && tag <= MAX_TAG);
    }

    #[test]
    fn test_cheri_tag_free() {
        let mut manager = CheriTagManager::new();
        let tag = manager.allocate_tag().unwrap();
        manager.free_tag(tag).unwrap();
    }

    #[test]
    fn test_capability_meta() {
        let meta = CapabilityMeta {
            base: 0x1000,
            length: 4096,
            offset: 0,
            permissions: permissions::READ_WRITE,
            tag: true,
        };
        assert_eq!(meta.base, 0x1000);
        assert_eq!(meta.length, 4096);
    }

    #[test]
    fn test_permissions() {
        assert_eq!(
            permissions::READ_WRITE,
            permissions::LOAD | permissions::STORE | permissions::LOAD_CAP | permissions::STORE_CAP
        );
    }
}
