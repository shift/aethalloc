//! Tests for size classification

use aethalloc_core::size_class::{round_up_pow2, slab_index, SizeClass};

#[test]
fn test_tiny_classification() {
    assert!(matches!(SizeClass::classify(16), SizeClass::Tiny));
    assert!(matches!(SizeClass::classify(128), SizeClass::Tiny));
}

#[test]
fn test_small_classification() {
    assert!(matches!(SizeClass::classify(256), SizeClass::Small));
    assert!(matches!(SizeClass::classify(8192), SizeClass::Small));
}

#[test]
fn test_medium_classification() {
    assert!(matches!(SizeClass::classify(16384), SizeClass::Medium));
    assert!(matches!(SizeClass::classify(262144), SizeClass::Medium));
}

#[test]
fn test_large_classification() {
    assert!(matches!(SizeClass::classify(262145), SizeClass::Large));
}

#[test]
fn test_round_up_pow2() {
    assert_eq!(round_up_pow2(1), 1);
    assert_eq!(round_up_pow2(3), 4);
    assert_eq!(round_up_pow2(1025), 2048);
}

#[test]
fn test_slab_index_valid() {
    assert_eq!(slab_index(16), Some(0));
    assert_eq!(slab_index(32), Some(1));
    assert_eq!(slab_index(64), Some(2));
    assert_eq!(slab_index(128), Some(3));
    assert_eq!(slab_index(256), Some(4));
    assert_eq!(slab_index(4096), Some(8));
    assert_eq!(slab_index(8192), Some(9));
}

#[test]
fn test_slab_index_invalid() {
    assert_eq!(slab_index(0), None);
    assert_eq!(slab_index(8), None);
    assert_eq!(slab_index(16384), None);
}
