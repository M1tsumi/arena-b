use std::alloc::Layout;
use std::ptr::NonNull;
use std::vec::Vec;

use crate::size_classes::SIZE_CLASSES;

#[repr(align(64))]
pub(crate) struct SlabAllocator {
    pools: [Vec<NonNull<u8>>; SIZE_CLASSES.len()],
}

impl SlabAllocator {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            pools: std::array::from_fn(|_| Vec::new()),
        }
    }

    #[inline]
    fn get_pool_index(size: usize) -> Option<usize> {
        SIZE_CLASSES
            .iter()
            .position(|&class_size| size <= class_size)
    }

    #[inline]
    pub(crate) unsafe fn alloc(&mut self, size: usize) -> Option<NonNull<u8>> {
        let pool_idx = Self::get_pool_index(size)?;
        self.pools[pool_idx].pop()
    }

    #[inline]
    pub(crate) unsafe fn dealloc(&mut self, ptr: NonNull<u8>, size: usize) {
        let Some(pool_idx) = Self::get_pool_index(size) else {
            if let Ok(layout) = Layout::from_size_align(size, 8) {
                std::alloc::dealloc(ptr.as_ptr(), layout);
            }
            return;
        };

        if self.pools[pool_idx].len() < 64 {
            self.pools[pool_idx].push(ptr);
        } else {
            if let Ok(layout) = Layout::from_size_align(size, 8) {
                std::alloc::dealloc(ptr.as_ptr(), layout);
            }
        }
    }
}
