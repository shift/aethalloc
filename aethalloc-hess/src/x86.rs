//! x86_64 pointer masking support
//!
//! Provides Intel LAM (Linear Address Masking) and AMD UAI (Upper Address Ignore)
//! support for hardware-enforced pointer tagging on x86_64.

use core::arch::x86_64::__cpuid;
use core::ptr::NonNull;

use crate::tag_manager::{Tag, TagError};

/// CPU feature flags for x86_64 pointer masking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X86FeatureFlags {
    pub has_intel_lam: bool,
    pub has_amd_uai: bool,
    pub has_la57: bool,
}

impl Default for X86FeatureFlags {
    fn default() -> Self {
        Self::detect()
    }
}

impl X86FeatureFlags {
    pub fn detect() -> Self {
        let mut flags = Self {
            has_intel_lam: false,
            has_amd_uai: false,
            has_la57: false,
        };

        let vendor = __cpuid(0);
        let vendor_bytes: [u8; 12] = [
            (vendor.ebx & 0xFF) as u8,
            ((vendor.ebx >> 8) & 0xFF) as u8,
            ((vendor.ebx >> 16) & 0xFF) as u8,
            ((vendor.ebx >> 24) & 0xFF) as u8,
            (vendor.edx & 0xFF) as u8,
            ((vendor.edx >> 8) & 0xFF) as u8,
            ((vendor.edx >> 16) & 0xFF) as u8,
            ((vendor.edx >> 24) & 0xFF) as u8,
            (vendor.ecx & 0xFF) as u8,
            ((vendor.ecx >> 8) & 0xFF) as u8,
            ((vendor.ecx >> 16) & 0xFF) as u8,
            ((vendor.ecx >> 24) & 0xFF) as u8,
        ];

        let is_intel = &vendor_bytes[..12] == b"GenuineIntel";
        let is_amd = &vendor_bytes[..12] == b"AuthenticAMD";

        if is_intel && vendor.eax >= 7 {
            let leaf7 = __cpuid(0x07);
            if (leaf7.edx & (1 << 26)) != 0 {
                flags.has_intel_lam = true;
            }
            if (leaf7.ecx & (1 << 16)) != 0 {
                flags.has_la57 = true;
            }
        } else if is_amd && vendor.eax >= 0x80000000 {
            let max_ext = __cpuid(0x80000000);
            if max_ext.eax >= 0x80000001 {
                let ext = __cpuid(0x80000001);
                if (ext.ecx & (1 << 6)) != 0 {
                    flags.has_amd_uai = true;
                }
            }
        }

        flags
    }

    pub fn has_hardware_tagging(&self) -> bool {
        self.has_intel_lam || self.has_amd_uai
    }
}

/// x86_64 tag manager using LAM/UAI or software fallback
pub struct X86TagManager {
    features: X86FeatureFlags,
    next_tag: Tag,
    used_tags: u16,
}

impl X86TagManager {
    pub fn new() -> Self {
        Self {
            features: X86FeatureFlags::detect(),
            next_tag: 1,
            used_tags: 0,
        }
    }

    pub fn features(&self) -> &X86FeatureFlags {
        &self.features
    }

    pub fn allocate_tag(&mut self) -> Result<Tag, TagError> {
        for _ in 1..=15 {
            let tag = self.next_tag;
            self.next_tag = if self.next_tag >= 15 {
                1
            } else {
                self.next_tag + 1
            };
            if (self.used_tags & (1 << tag)) == 0 {
                self.used_tags |= 1 << tag;
                return Ok(tag);
            }
        }
        Err(TagError::OutOfTags)
    }

    pub fn free_tag(&mut self, tag: Tag) -> Result<(), TagError> {
        if tag == 0 || tag > 15 {
            return Err(TagError::InvalidTag);
        }
        self.used_tags &= !(1 << tag);
        Ok(())
    }

    pub fn tag_pointer(&self, ptr: NonNull<u8>, tag: Tag) -> NonNull<u8> {
        if tag > 15 {
            return ptr;
        }
        let addr = ptr.as_ptr() as usize;
        let tagged = if self.features.has_hardware_tagging() {
            (addr & 0x00FFFFFFFFFFFFFF) | ((tag as usize) << 56)
        } else {
            (addr & 0x0000FFFFFFFFFFFF) | ((tag as usize) << 48)
        };
        unsafe { NonNull::new_unchecked(tagged as *mut u8) }
    }

    pub fn untag_pointer(&self, ptr: NonNull<u8>) -> NonNull<u8> {
        let addr = ptr.as_ptr() as usize;
        let untagged = if self.features.has_hardware_tagging() {
            addr & 0x00FFFFFFFFFFFFFF
        } else {
            addr & 0x0000FFFFFFFFFFFF
        };
        unsafe { NonNull::new_unchecked(untagged as *mut u8) }
    }

    pub fn get_tag(&self, ptr: NonNull<u8>) -> Tag {
        let addr = ptr.as_ptr() as usize;
        if self.features.has_hardware_tagging() {
            ((addr >> 56) & 0xF) as Tag
        } else {
            ((addr >> 48) & 0xF) as Tag
        }
    }
}

impl Default for X86TagManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_detection() {
        let flags = X86FeatureFlags::detect();
        let _ = flags.has_hardware_tagging();
    }

    #[test]
    fn test_tag_manager() {
        let mut mgr = X86TagManager::default();
        let tag = mgr.allocate_tag().unwrap();
        assert!(tag >= 1 && tag <= 15);
    }
}
