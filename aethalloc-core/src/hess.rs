//! HESS integration - Hardware-Enforced Spatial Safety
//!
//! Provides memory tagging for allocations using:
//! - SoftwareTagManager (default fallback)
//! - ARM MTE (with `mte` feature)
//! - CHERI capabilities (with `cheri` feature)

use core::ptr::NonNull;

#[cfg(feature = "hess")]
pub use aethalloc_hess::tag_manager::{
    SoftwareTagManager, Tag, TagError, TagManager, TaggedAllocation, MAX_TAG, MIN_TAG,
};

#[cfg(all(feature = "mte", target_arch = "aarch64"))]
pub use aethalloc_hess::mte::MteTagManager;

#[cfg(feature = "cheri")]
pub use aethalloc_hess::cheri::CheriTagManager;

#[cfg(not(feature = "hess"))]
pub type Tag = u16;
#[cfg(not(feature = "hess"))]
pub const MAX_TAG: Tag = 0;
#[cfg(not(feature = "hess"))]
pub const MIN_TAG: Tag = 0;

#[cfg(not(feature = "hess"))]
#[derive(Debug, Clone, Copy)]
pub struct TaggedAllocation {
    pub ptr: NonNull<u8>,
    pub size: usize,
    pub tag: Tag,
}

#[cfg(not(feature = "hess"))]
impl TaggedAllocation {
    pub fn new(ptr: NonNull<u8>, size: usize, tag: Tag) -> Self {
        Self { ptr, size, tag }
    }
}

#[cfg(feature = "hess")]
type TagManagerImpl = SoftwareTagManager;

#[cfg(all(feature = "mte", target_arch = "aarch64"))]
type TagManagerImpl = MteTagManager;

#[cfg(feature = "cheri")]
type TagManagerImpl = CheriTagManager;

fn create_tag_manager() -> TagManagerImpl {
    TagManagerImpl::new()
}

/// Tag a memory region and return the tagged pointer
///
/// Uses the best available tagging mechanism for the current platform.
/// Falls back to software tagging on unsupported platforms.
///
/// # Safety
/// - ptr must point to valid allocated memory
/// - size must match the allocation size
#[inline]
pub unsafe fn tag_allocation(ptr: NonNull<u8>, size: usize) -> TaggedAllocation {
    #[cfg(feature = "hess")]
    {
        let mut mgr = create_tag_manager();
        match mgr.allocate_tag() {
            Ok(tag) => {
                let _ = mgr.store_tag(ptr, tag);
                let tagged_ptr = mgr.tag_pointer(ptr, tag).unwrap_or(ptr);
                TaggedAllocation::new(tagged_ptr, size, tag)
            }
            Err(_) => TaggedAllocation::new(ptr, size, 0),
        }
    }
    #[cfg(not(feature = "hess"))]
    {
        TaggedAllocation::new(ptr, size, 0)
    }
}

/// Verify the tag on a pointer matches the expected tag
///
/// Returns true if the tag is valid, false if corruption detected.
///
/// # Safety
/// - ptr must point to valid memory
#[inline]
pub unsafe fn verify_tag(ptr: NonNull<u8>, expected_tag: Tag) -> bool {
    #[cfg(feature = "hess")]
    {
        let mgr = create_tag_manager();
        let actual_tag = mgr.get_tag(ptr);
        actual_tag == expected_tag
    }
    #[cfg(not(feature = "hess"))]
    {
        let _ = (ptr, expected_tag);
        true
    }
}
