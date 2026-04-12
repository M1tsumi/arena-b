//! Virtual memory strategy for large arena allocations

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use core::ptr;
const PAGE_SIZE: usize = 4096;
const DEFAULT_RESERVE_SIZE: usize = 16 * 1024 * 1024; // 16MB
const DEFAULT_COMMIT_SIZE: usize = 64 * 1024; // 64KB
const MAX_RESERVE_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB safe default

// Virtual memory region using reserve/commit pattern
pub struct VirtualMemoryRegion {
    pub ptr: *mut u8,
    pub reserved_size: usize,
    pub committed_size: usize,
}

impl VirtualMemoryRegion {
    pub fn new(reserve_size: usize) -> Result<Self, &'static str> {
        if reserve_size == 0 {
            return Err("Reserve size must be nonzero");
        }
        let reserve_size = reserve_size.clamp(PAGE_SIZE, MAX_RESERVE_SIZE);
        let reserve_size = (reserve_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        let ptr = unsafe {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Memory::{
                    VirtualAlloc, MEM_RESERVE, PAGE_READWRITE,
                };
                VirtualAlloc(ptr::null_mut(), reserve_size, MEM_RESERVE, PAGE_READWRITE)
            }
            #[cfg(unix)]
            {
                libc::mmap(
                    ptr::null_mut(),
                    reserve_size,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            }
        };

        #[cfg(windows)]
        {
            if ptr.is_null() {
                return Err("Failed to reserve virtual memory");
            }
        }
        #[cfg(unix)]
        {
            if std::ptr::eq(ptr, libc::MAP_FAILED as *mut _) {
                return Err("Failed to reserve virtual memory");
            }
        }

        Ok(Self {
            ptr: ptr as *mut u8,
            reserved_size: reserve_size,
            committed_size: 0,
        })
    }

    pub fn commit(&mut self, offset: usize, size: usize) -> Result<(), &'static str> {
        if size == 0 {
            return Ok(());
        }

        let offset = (offset + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = offset.checked_add(size).ok_or("Commit range overflow")?;

        if end > self.reserved_size {
            return Err("Commit size exceeds reserved size");
        }

        let commit_ptr = unsafe { self.ptr.add(offset) };

        unsafe {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Memory::{
                    VirtualAlloc, MEM_COMMIT, PAGE_READWRITE,
                };
                let result = VirtualAlloc(commit_ptr as *mut _, size, MEM_COMMIT, PAGE_READWRITE);
                if result.is_null() {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    return Err(match err {
                        windows_sys::Win32::Foundation::ERROR_NOT_ENOUGH_MEMORY => {
                            "Insufficient virtual memory during commit"
                        }
                        _ => "Failed to commit virtual memory",
                    });
                }
            }
            #[cfg(unix)]
            {
                let result = libc::mprotect(
                    commit_ptr as *mut libc::c_void,
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                );
                if result != 0 {
                    return Err("Failed to commit virtual memory");
                }

                #[cfg(target_os = "macos")]
                {
                    libc::pthread_jit_write_protect_np(0);
                }
                #[cfg(target_os = "macos")]
                {
                    libc::pthread_jit_write_protect_np(1);
                }
            }
        }

        self.committed_size = self.committed_size.max(end);
        Ok(())
    }

    pub fn decommit(&mut self, offset: usize, size: usize) -> Result<(), &'static str> {
        if size == 0 {
            return Ok(());
        }

        if offset >= self.reserved_size {
            return Err("Offset exceeds reserved size");
        }

        let offset = (offset + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = offset.checked_add(size).ok_or("Decommit range overflow")?;

        if end > self.committed_size {
            return Err("Decommit size exceeds committed size");
        }

        // Ensure size is a multiple of page size
        if !size.is_multiple_of(PAGE_SIZE) {
            return Err("Size must be a multiple of page size");
        }

        let decommit_ptr = unsafe { self.ptr.add(offset) };

        if decommit_ptr.is_null() {
            return Err("Invalid pointer for decommit");
        }

        unsafe {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Memory::{VirtualFree, MEM_DECOMMIT};
                let result = VirtualFree(decommit_ptr as *mut _, size, MEM_DECOMMIT);
                if result == 0 {
                    return Err("Failed to decommit virtual memory");
                }
            }
            #[cfg(unix)]
            {
                let result =
                    libc::mprotect(decommit_ptr as *mut libc::c_void, size, libc::PROT_NONE);
                if result != 0 {
                    return Err("Failed to decommit virtual memory");
                }

                // Also discard the pages to free physical memory
                #[cfg(target_os = "macos")]
                {
                    libc::madvise(decommit_ptr as *mut libc::c_void, size, libc::MADV_FREE);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    libc::madvise(decommit_ptr as *mut libc::c_void, size, libc::MADV_DONTNEED);
                }
            }
        }

        // If we're decommitting the end of the committed region, shrink it
        if end == self.committed_size {
            self.committed_size = offset;
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        if self.committed_size > 0 {
            let _ = self.decommit(0, self.committed_size);
            self.committed_size = 0;
        }
    }

    pub fn committed_bytes(&self) -> usize {
        self.committed_size
    }
}

impl Drop for VirtualMemoryRegion {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                #[cfg(windows)]
                {
                    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
                    let _ = VirtualFree(self.ptr as *mut _, 0, MEM_RELEASE);
                }
                #[cfg(unix)]
                {
                    let _ = libc::munmap(self.ptr as *mut libc::c_void, self.reserved_size);
                }
            }
        }
    }
}

// Virtual chunk that uses virtual memory region
pub struct VirtualChunk {
    region: VirtualMemoryRegion,
    capacity: usize,
    used: std::sync::atomic::AtomicUsize,
}

impl VirtualChunk {
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        let mut region = VirtualMemoryRegion::new(capacity)?;

        // Commit initial chunk
        let initial_commit = capacity.min(DEFAULT_COMMIT_SIZE);
        region.commit(0, initial_commit)?;

        Ok(Self {
            region,
            capacity,
            used: core::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// # Safety
    ///
    /// The caller must ensure that `layout` is valid and that the returned pointer
    /// is used in a manner consistent with the specified `layout`. The pointer must
    /// not be dereferenced beyond the committed region, and alignment requirements
    /// must be respected.
    pub unsafe fn allocate(&self, layout: alloc::alloc::Layout) -> Option<*mut u8> {
        let size = layout.size();
        let align = layout.align();

        let current_used = self.used.load(core::sync::atomic::Ordering::Acquire);
        let start = (current_used + align - 1) & !(align - 1);
        let end = start + size;

        if end > self.capacity {
            return None;
        }

        // Ensure the memory is committed
        if end > self.region.committed_size {
            let additional_size = end - self.region.committed_size;
            if (*(&self.region as *const _ as *mut VirtualMemoryRegion))
                .commit(self.region.committed_size, additional_size)
                .is_err()
            {
                return None;
            }
        }

        if self
            .used
            .compare_exchange_weak(
                current_used,
                end,
                core::sync::atomic::Ordering::AcqRel,
                core::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            let ptr = self.region.ptr.add(start);
            return Some(ptr);
        }

        None
    }

    pub fn reset(&mut self) {
        self.used.store(0, core::sync::atomic::Ordering::Release);
        self.region.reset();
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn used(&self) -> usize {
        self.used.load(core::sync::atomic::Ordering::Acquire)
    }
}
