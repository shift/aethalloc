//! Size classification for allocation requests

/// Size classes for allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    /// 16-128 bytes, power-of-2
    Tiny,
    /// 256B - 8KB, slab allocation
    Small,
    /// 16KB - 256KB, buddy system
    Medium,
    /// >256KB, direct mmap
    Large,
}

impl SizeClass {
    /// Classify a size request into a size class
    pub fn classify(size: usize) -> Self {
        if size == 0 {
            return SizeClass::Tiny;
        }

        if size <= 128 {
            SizeClass::Tiny
        } else if size <= 8 * 1024 {
            SizeClass::Small
        } else if size <= 256 * 1024 {
            SizeClass::Medium
        } else {
            SizeClass::Large
        }
    }

    /// Get the allocation size for this class (rounded up to class boundary)
    pub fn alloc_size(&self, requested: usize) -> usize {
        match self {
            SizeClass::Tiny => {
                if requested == 0 {
                    return 16;
                }
                let pow2 = round_up_pow2(requested);
                if pow2 < 16 {
                    16
                } else {
                    pow2
                }
            }
            SizeClass::Small => round_up_pow2(requested),
            SizeClass::Medium => round_up_pow2(requested),
            SizeClass::Large => {
                let page_size = crate::page::PAGE_SIZE;
                requested.div_ceil(page_size) * page_size
            }
        }
    }
}

/// Round up to next power of 2
pub fn round_up_pow2(size: usize) -> usize {
    if size == 0 {
        return 1;
    }
    if size.is_power_of_two() {
        return size;
    }
    1usize << (usize::BITS - size.leading_zeros())
}

/// Get the slab slot index for a tiny/small allocation
///
/// Returns an index 0-15 for sizes 16B through 8KB (power of 2)
pub fn slab_index(size: usize) -> Option<usize> {
    if size == 0 || size > 8 * 1024 {
        return None;
    }

    let alloc_size = round_up_pow2(size);
    if alloc_size < 16 {
        return None;
    }

    let idx = alloc_size.trailing_zeros() as usize;
    if (4..=13).contains(&idx) {
        Some(idx - 4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_zero() {
        assert_eq!(SizeClass::classify(0), SizeClass::Tiny);
    }

    #[test]
    fn test_classify_tiny() {
        assert_eq!(SizeClass::classify(1), SizeClass::Tiny);
        assert_eq!(SizeClass::classify(16), SizeClass::Tiny);
        assert_eq!(SizeClass::classify(64), SizeClass::Tiny);
        assert_eq!(SizeClass::classify(128), SizeClass::Tiny);
    }

    #[test]
    fn test_classify_small() {
        assert_eq!(SizeClass::classify(129), SizeClass::Small);
        assert_eq!(SizeClass::classify(256), SizeClass::Small);
        assert_eq!(SizeClass::classify(4096), SizeClass::Small);
        assert_eq!(SizeClass::classify(8192), SizeClass::Small);
    }

    #[test]
    fn test_classify_medium() {
        assert_eq!(SizeClass::classify(8193), SizeClass::Medium);
        assert_eq!(SizeClass::classify(16384), SizeClass::Medium);
        assert_eq!(SizeClass::classify(262144), SizeClass::Medium);
    }

    #[test]
    fn test_classify_large() {
        assert_eq!(SizeClass::classify(262145), SizeClass::Large);
        assert_eq!(SizeClass::classify(1024 * 1024), SizeClass::Large);
    }

    #[test]
    fn test_round_up_pow2() {
        assert_eq!(round_up_pow2(0), 1);
        assert_eq!(round_up_pow2(1), 1);
        assert_eq!(round_up_pow2(2), 2);
        assert_eq!(round_up_pow2(3), 4);
        assert_eq!(round_up_pow2(4), 4);
        assert_eq!(round_up_pow2(5), 8);
        assert_eq!(round_up_pow2(1023), 1024);
        assert_eq!(round_up_pow2(1024), 1024);
        assert_eq!(round_up_pow2(1025), 2048);
    }

    #[test]
    fn test_slab_index() {
        assert_eq!(slab_index(16), Some(0));
        assert_eq!(slab_index(32), Some(1));
        assert_eq!(slab_index(64), Some(2));
        assert_eq!(slab_index(128), Some(3));
        assert_eq!(slab_index(256), Some(4));
        assert_eq!(slab_index(512), Some(5));
        assert_eq!(slab_index(1024), Some(6));
        assert_eq!(slab_index(2048), Some(7));
        assert_eq!(slab_index(4096), Some(8));
        assert_eq!(slab_index(8192), Some(9));
        assert_eq!(slab_index(0), None);
        assert_eq!(slab_index(8), None);
        assert_eq!(slab_index(16384), None);
    }
}
