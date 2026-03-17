//! Tests for global_pool module

use aethalloc_core::global_pool::GlobalPools;
use core::ptr::NonNull;
use std::alloc::{alloc, dealloc, Layout};

#[test]
fn test_global_pool_push_pop_single() {
    let pools = GlobalPools::new();
    let layout = Layout::from_size_align(64, 8).unwrap();

    unsafe {
        let ptr = alloc(layout);
        assert!(!ptr.is_null());
        let nn = NonNull::new_unchecked(ptr);

        pools.push(0, nn);

        let popped = pools.pop(0);
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().as_ptr(), ptr);

        dealloc(ptr, layout);
    }
}

#[test]
fn test_global_pool_pop_empty() {
    let pools = GlobalPools::new();

    assert!(pools.pop(0).is_none());
    assert!(pools.pop(5).is_none());
    assert!(pools.pop(15).is_none());
}

#[test]
fn test_global_pool_multiple_push_pop() {
    let pools = GlobalPools::new();
    let layout = Layout::from_size_align(64, 8).unwrap();

    unsafe {
        let ptrs: Vec<*mut u8> = (0..10).map(|_| alloc(layout)).collect();

        for &ptr in &ptrs {
            pools.push(0, NonNull::new_unchecked(ptr));
        }

        let mut popped_count = 0;
        while let Some(_) = pools.pop(0) {
            popped_count += 1;
        }

        assert_eq!(popped_count, 10);

        for &ptr in &ptrs {
            dealloc(ptr, layout);
        }
    }
}

#[test]
fn test_global_pool_different_size_classes() {
    let pools = GlobalPools::new();
    let layout = Layout::from_size_align(64, 8).unwrap();

    unsafe {
        let ptr0 = NonNull::new_unchecked(alloc(layout));
        let ptr5 = NonNull::new_unchecked(alloc(layout));
        let ptr10 = NonNull::new_unchecked(alloc(layout));

        pools.push(0, ptr0);
        pools.push(5, ptr5);
        pools.push(10, ptr10);

        assert!(pools.pop(0).is_some());
        assert!(pools.pop(5).is_some());
        assert!(pools.pop(10).is_some());

        dealloc(ptr0.as_ptr(), layout);
        dealloc(ptr5.as_ptr(), layout);
        dealloc(ptr10.as_ptr(), layout);
    }
}

#[test]
fn test_global_pool_lifo_order() {
    let pools = GlobalPools::new();
    let layout = Layout::from_size_align(64, 8).unwrap();

    unsafe {
        let ptr1 = NonNull::new_unchecked(alloc(layout));
        let ptr2 = NonNull::new_unchecked(alloc(layout));
        let ptr3 = NonNull::new_unchecked(alloc(layout));

        pools.push(0, ptr1);
        pools.push(0, ptr2);
        pools.push(0, ptr3);

        // LIFO: should get ptr3, ptr2, ptr1
        assert_eq!(pools.pop(0).unwrap().as_ptr(), ptr3.as_ptr());
        assert_eq!(pools.pop(0).unwrap().as_ptr(), ptr2.as_ptr());
        assert_eq!(pools.pop(0).unwrap().as_ptr(), ptr1.as_ptr());

        dealloc(ptr1.as_ptr(), layout);
        dealloc(ptr2.as_ptr(), layout);
        dealloc(ptr3.as_ptr(), layout);
    }
}
