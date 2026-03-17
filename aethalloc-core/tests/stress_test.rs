//! Stress tests for AethAlloc core allocator
//!
//! These tests exercise the allocator under heavy load to verify
//! correctness and detect potential race conditions or memory leaks.

use aethalloc_core::buddy::BuddyAllocator;
use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::SizeClass;
use aethalloc_core::slab::Slab;
use aethalloc_core::thread_local::ThreadLocalCache;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

#[test]
fn test_stress_slab_allocator() {
    let mut slab = Slab::new(64);
    let mut ptrs = Vec::new();

    for _ in 0..1000 {
        if let Some(ptr) = slab.alloc() {
            ptrs.push(ptr);
        }
    }

    assert!(!ptrs.is_empty());

    for ptr in ptrs {
        unsafe {
            slab.dealloc(ptr);
        }
    }
}

#[test]
fn test_stress_buddy_allocator() {
    let mut buddy = BuddyAllocator::new();

    let sizes = [16 * 1024, 32 * 1024, 64 * 1024, 128 * 1024];
    let mut allocations = Vec::new();

    unsafe {
        let region = PageAllocator::alloc(64).expect("Failed to allocate region");
        buddy.init(region, 64 * 4096);
    }

    for _ in 0..10 {
        for &size in &sizes {
            if let Some(ptr) = buddy.alloc(size) {
                allocations.push((ptr, size));
            }
        }
    }

    for (ptr, size) in allocations {
        unsafe {
            buddy.dealloc(ptr, size);
        }
    }
}

#[test]
fn test_stress_thread_local_cache() {
    let cache = ThreadLocalCache::new();

    let mut ptrs = Vec::new();

    for size in 16..=4096 {
        if let Some(ptr) = cache.alloc(size) {
            ptrs.push((ptr, size));
        }
    }

    for (ptr, size) in ptrs {
        unsafe {
            cache.dealloc(ptr, size);
        }
    }
}

#[test]
fn test_stress_concurrent_allocations() {
    let alloc_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for _ in 0..4 {
        let alloc_count = Arc::clone(&alloc_count);
        let handle = thread::spawn(move || {
            let cache = ThreadLocalCache::new();
            let mut ptrs = Vec::new();

            for _ in 0..100 {
                for size in [32, 64, 128, 256, 512].iter() {
                    if let Some(ptr) = cache.alloc(*size) {
                        ptrs.push((ptr, *size));
                        alloc_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            for (ptr, size) in ptrs {
                unsafe {
                    cache.dealloc(ptr, size);
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert!(alloc_count.load(Ordering::Relaxed) > 0);
}

#[test]
fn test_stress_page_allocator() {
    let mut pages = Vec::new();

    unsafe {
        for _ in 0..100 {
            if let Some(ptr) = PageAllocator::alloc(1) {
                pages.push(ptr);
            }
        }

        for ptr in pages {
            PageAllocator::dealloc(ptr, 1);
        }
    }
}

#[test]
fn test_stress_fragmentation() {
    let mut slab = Slab::new(128);

    let mut all_ptrs = Vec::new();
    for _ in 0..32 {
        if let Some(ptr) = slab.alloc() {
            all_ptrs.push(ptr);
        }
    }

    let mut keep_ptrs = Vec::new();
    for (i, ptr) in all_ptrs.into_iter().enumerate() {
        if i % 2 == 0 {
            keep_ptrs.push(ptr);
        } else {
            unsafe {
                slab.dealloc(ptr);
            }
        }
    }

    for _ in 0..16 {
        if let Some(ptr) = slab.alloc() {
            keep_ptrs.push(ptr);
        }
    }

    for ptr in keep_ptrs {
        unsafe {
            slab.dealloc(ptr);
        }
    }
}

#[test]
fn test_size_class_boundaries() {
    assert_eq!(SizeClass::classify(16), SizeClass::Tiny);
    assert_eq!(SizeClass::classify(128), SizeClass::Tiny);
    assert_eq!(SizeClass::classify(256), SizeClass::Small);
    assert_eq!(SizeClass::classify(8192), SizeClass::Small);
    assert_eq!(SizeClass::classify(16384), SizeClass::Medium);
    assert_eq!(SizeClass::classify(262144), SizeClass::Medium);
    assert_eq!(SizeClass::classify(262145), SizeClass::Large);
}

#[test]
fn test_alignment_requirements() {
    unsafe {
        for &align in &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            if let Some(ptr) = PageAllocator::alloc(1) {
                let addr = ptr.as_ptr() as usize;
                assert_eq!(addr % align, 0, "Alignment {} not satisfied", align);
                PageAllocator::dealloc(ptr, 1);
            }
        }
    }
}

#[test]
fn test_zero_allocations() {
    let cache = ThreadLocalCache::new();
    let ptr = cache.alloc(0);
    assert!(ptr.is_some());
    if let Some(p) = ptr {
        unsafe {
            cache.dealloc(p, 0);
        }
    }
}

#[test]
fn test_large_allocations() {
    let cache = ThreadLocalCache::new();

    assert!(cache.alloc(1024 * 1024).is_none());
    assert!(cache.alloc(10 * 1024 * 1024).is_none());
}
