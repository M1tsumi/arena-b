//! Memory safety debugging with guards and use-after-rewind detection

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

pub const GUARD_MAGIC: u64 = 0xDEADBEEFCAFEBABE;
pub const FREED_MAGIC: u64 = 0xFEEDFACECAFEBABE;
const GUARD_SIZE: usize = 16;

// Debug guard values for corruption detection
#[repr(C)]
#[derive(Debug)]
struct DebugGuard {
    pre_guard: [u8; GUARD_SIZE],
    post_guard: [u8; GUARD_SIZE],
    magic: u64,
    size: usize,
    checkpoint_id: usize,
}

impl DebugGuard {
    fn new(size: usize, checkpoint_id: usize) -> Self {
        let mut guard = Self {
            pre_guard: [0xCC; GUARD_SIZE],
            post_guard: [0xCC; GUARD_SIZE],
            magic: GUARD_MAGIC,
            size,
            checkpoint_id,
        };

        // Write magic pattern to guards using byte pattern to avoid large shifts
        let bytes = GUARD_MAGIC.to_ne_bytes();
        for i in 0..GUARD_SIZE {
            guard.pre_guard[i] = bytes[i % bytes.len()];
            guard.post_guard[i] = bytes[i % bytes.len()];
        }

        guard
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.magic != GUARD_MAGIC {
            return Err("Magic number corrupted");
        }

        let bytes = GUARD_MAGIC.to_ne_bytes();
        for i in 0..GUARD_SIZE {
            let expected = bytes[i % bytes.len()];
            if self.pre_guard[i] != expected {
                return Err("Pre-guard corrupted");
            }
            if self.post_guard[i] != expected {
                return Err("Post-guard corrupted");
            }
        }

        Ok(())
    }
}

// Allocation tracking for use-after-rewind detection
#[derive(Debug, Clone)]
pub struct AllocationInfo {
    ptr: *mut u8,
    size: usize,
    checkpoint_id: usize,
    captured_backtrace: Option<String>,
}

unsafe impl Send for AllocationInfo {}
unsafe impl Sync for AllocationInfo {}

// Global debug state
pub static DEBUG_STATE: LazyLock<RwLock<DebugState>> =
    LazyLock::new(|| RwLock::new(DebugState::new()));
static VALIDATION_ENABLED: AtomicBool = AtomicBool::new(false);

pub struct DebugState {
    allocations: HashMap<usize, Vec<AllocationInfo>>,
    arena_checkpoints: HashMap<usize, usize>,
    next_arena_id: AtomicUsize,
    corrupted_allocations: usize,
    leak_reports: usize,
    backtraces_enabled: bool,
}

impl DebugState {
    fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            arena_checkpoints: HashMap::new(),
            next_arena_id: AtomicUsize::new(1),
            corrupted_allocations: 0,
            leak_reports: 0,
            backtraces_enabled: cfg!(feature = "debug"),
        }
    }

    fn register_allocation(
        &mut self,
        arena_id: usize,
        ptr: *mut u8,
        size: usize,
        checkpoint_id: usize,
    ) {
        let info = AllocationInfo {
            ptr,
            size,
            checkpoint_id,
            captured_backtrace: self.capture_backtrace(),
        };

        self.allocations
            .entry(arena_id)
            .or_insert_with(Vec::new)
            .push(info);

        self.arena_checkpoints.insert(arena_id, checkpoint_id);
    }

    fn validate_allocation(&self, arena_id: usize, ptr: *mut u8) -> Result<(), &'static str> {
        if let Some(allocations) = self.allocations.get(&arena_id) {
            for info in allocations {
                if info.ptr == ptr {
                    // Check if allocation is from a valid checkpoint
                    if let Some(&current_checkpoint) = self.arena_checkpoints.get(&arena_id) {
                        if info.checkpoint_id > current_checkpoint {
                            return Err("Use-after-rewind detected");
                        }
                    }

                    // Validate guard bytes (use unaligned read to avoid UB)
                    let guard_ptr = unsafe { ptr.sub(GUARD_SIZE) };
                    let guard_val =
                        unsafe { core::ptr::read_unaligned(guard_ptr as *const DebugGuard) };

                    return guard_val.validate();
                }
            }
        }

        Err("Allocation not found")
    }

    fn validate_arena(&self, arena_id: usize) -> Result<(), String> {
        if let Some(allocations) = self.allocations.get(&arena_id) {
            let mut reports = Vec::new();
            for info in allocations {
                if let Err(err) = self.validate_allocation(arena_id, info.ptr) {
                    let trace = info
                        .captured_backtrace
                        .as_deref()
                        .unwrap_or("<backtrace unavailable>");
                    reports.push(format!("Pointer {:p}: {}\n{}", info.ptr, err, trace));
                }
            }

            if reports.is_empty() {
                Ok(())
            } else {
                Err(reports.join("\n"))
            }
        } else {
            Err("Arena not found".into())
        }
    }

    fn capture_backtrace(&self) -> Option<String> {
        if !self.backtraces_enabled {
            return None;
        }

        #[cfg(feature = "debug")]
        {
            Some(std::backtrace::Backtrace::capture().to_string())
        }

        #[cfg(not(feature = "debug"))]
        {
            None
        }
    }

    fn leak_report(&mut self, arena_id: usize) -> Vec<String> {
        let mut reports = Vec::new();
        if let Some(allocations) = self.allocations.get(&arena_id) {
            for info in allocations {
                let trace = info
                    .captured_backtrace
                    .as_deref()
                    .unwrap_or("<backtrace unavailable>");
                reports.push(format!(
                    "Leaked allocation: ptr={:p}, size={}, checkpoint={}\n{}",
                    info.ptr, info.size, info.checkpoint_id, trace
                ));
            }
        }
        self.leak_reports += reports.len();
        reports
    }

    fn rewind_to_checkpoint(&mut self, arena_id: usize, checkpoint_id: usize) {
        self.arena_checkpoints.insert(arena_id, checkpoint_id);

        // Mark allocations from future checkpoints as invalid
        if let Some(allocations) = self.allocations.get_mut(&arena_id) {
            for info in allocations.iter_mut() {
                if info.checkpoint_id > checkpoint_id {
                    // Corrupt the guard to detect use-after-rewind (write unaligned)
                    unsafe {
                        let guard_u8 = info.ptr.sub(GUARD_SIZE);
                        // Taint the first guard byte to mark corruption without causing aligned deref
                        core::ptr::write_unaligned(guard_u8, 0u8);
                    }
                }
            }
        }
    }

    pub fn get_stats(&self, arena_id: usize) -> (usize, usize) {
        let total = self
            .allocations
            .get(&arena_id)
            .map(|allocs| allocs.len())
            .unwrap_or(0);

        let corrupted = self.corrupted_allocations;

        (total, corrupted)
    }

    pub fn get_current_checkpoint_id(&self, arena_id: usize) -> usize {
        self.arena_checkpoints.get(&arena_id).copied().unwrap_or(0)
    }

    pub fn check_use_after_rewind(&self, arena_id: usize, ptr: *mut u8) -> Result<(), String> {
        self.validate_allocation(arena_id, ptr)
            .map_err(|e| e.to_string())
    }

    fn cleanup_arena(&mut self, arena_id: usize) {
        self.allocations.remove(&arena_id);
        self.arena_checkpoints.remove(&arena_id);
    }
}

// Public debug interface
pub fn register_allocation(arena_id: usize, ptr: *mut u8, size: usize, checkpoint_id: usize) {
    if let Ok(mut state) = DEBUG_STATE.write() {
        state.register_allocation(arena_id, ptr, size, checkpoint_id);
    }
}

pub fn validate_allocation(arena_id: usize, ptr: *mut u8) -> Result<(), &'static str> {
    if !VALIDATION_ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }

    if let Ok(state) = DEBUG_STATE.read() {
        state.validate_allocation(arena_id, ptr)
    } else {
        Err("Debug state locked")
    }
}

pub fn rewind_to_checkpoint(checkpoint_id: usize) {
    if let Ok(mut state) = DEBUG_STATE.write() {
        // Find all arenas and rewind them to the checkpoint
        let arena_ids: Vec<usize> = state.arena_checkpoints.keys().copied().collect();
        for arena_id in arena_ids {
            state.rewind_to_checkpoint(arena_id, checkpoint_id);
        }
    }
}

pub fn rewind_arena_to_checkpoint(arena_id: usize, checkpoint_id: usize) {
    if let Ok(mut state) = DEBUG_STATE.write() {
        state.rewind_to_checkpoint(arena_id, checkpoint_id);
    }
}

pub fn validate_arena(arena_id: usize) -> Result<(), String> {
    if !VALIDATION_ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }

    if let Ok(state) = DEBUG_STATE.read() {
        state.validate_arena(arena_id)
    } else {
        Err("Debug state locked".into())
    }
}

pub fn enable_validation(enable: bool) {
    VALIDATION_ENABLED.store(enable, Ordering::Relaxed);
}

pub fn leak_report(arena_id: usize) -> Vec<String> {
    if let Ok(mut state) = DEBUG_STATE.write() {
        state.leak_report(arena_id)
    } else {
        vec!["Debug state locked".into()]
    }
}

pub fn get_debug_stats() -> crate::core::DebugStats {
    if let Ok(state) = DEBUG_STATE.read() {
        crate::core::DebugStats {
            total_allocations: state.allocations.values().map(|v| v.len()).sum(),
            active_checkpoints: state.arena_checkpoints.len(),
            current_checkpoint_id: state.arena_checkpoints.values().max().copied().unwrap_or(0),
            corrupted_allocations: state.corrupted_allocations,
            leak_reports: state.leak_reports,
        }
    } else {
        crate::core::DebugStats::default()
    }
}

pub fn cleanup_arena(arena_id: usize) {
    if let Ok(mut state) = DEBUG_STATE.write() {
        state.cleanup_arena(arena_id);
    }
}

// Debug-enabled allocator wrapper
pub struct DebugAllocator {
    arena_id: usize,
    enabled: bool,
}

impl DebugAllocator {
    pub fn new() -> Self {
        let arena_id = if let Ok(state) = DEBUG_STATE.read() {
            state.next_arena_id.fetch_add(1, Ordering::Relaxed)
        } else {
            1
        };

        Self {
            arena_id,
            enabled: true,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn arena_id(&self) -> usize {
        self.arena_id
    }

    /// # Safety
    ///
    /// `ptr` must be a valid, non-null pointer to `size` bytes of initialized data
    /// allocated by the arena. The caller must ensure `size` is correct and that
    /// the memory is valid for reads and writes during guard wrapping.
    pub unsafe fn allocate_with_guard(
        &self,
        ptr: *mut u8,
        size: usize,
        checkpoint_id: usize,
    ) -> *mut u8 {
        if !self.enabled {
            return ptr;
        }

        let total_size = size + 2 * GUARD_SIZE + core::mem::size_of::<DebugGuard>();
        let layout = match Layout::from_size_align(total_size, 16) {
            Ok(l) => l,
            Err(_) => {
                eprintln!(
                    "DebugAllocator::allocate_with_guard: invalid layout total_size={}",
                    total_size
                );
                return ptr; // Fallback to original allocation when debug allocation can't be made
            }
        };

        let debug_ptr = unsafe { alloc(layout) };
        if debug_ptr.is_null() {
            return ptr; // Fallback to original allocation
        }

        // Create debug guard
        let guard = DebugGuard::new(size, checkpoint_id);

        unsafe {
            // Copy guard to the beginning
            core::ptr::copy_nonoverlapping(
                &guard as *const DebugGuard as *const u8,
                debug_ptr,
                core::mem::size_of::<DebugGuard>(),
            );

            // Copy pre-guard
            core::ptr::copy_nonoverlapping(
                guard.pre_guard.as_ptr(),
                debug_ptr.add(core::mem::size_of::<DebugGuard>()),
                GUARD_SIZE,
            );

            // Copy actual data
            core::ptr::copy_nonoverlapping(
                ptr,
                debug_ptr.add(core::mem::size_of::<DebugGuard>() + GUARD_SIZE),
                size,
            );

            // Copy post-guard
            core::ptr::copy_nonoverlapping(
                guard.post_guard.as_ptr(),
                debug_ptr.add(core::mem::size_of::<DebugGuard>() + GUARD_SIZE + size),
                GUARD_SIZE,
            );

            // Register allocation
            register_allocation(
                self.arena_id,
                debug_ptr.add(core::mem::size_of::<DebugGuard>() + GUARD_SIZE),
                size,
                checkpoint_id,
            );
        }

        // Do not deallocate the original pointer here — it may belong to arena chunks.

        unsafe { debug_ptr.add(core::mem::size_of::<DebugGuard>() + GUARD_SIZE) }
    }

    pub fn validate_pointer(&self, ptr: *mut u8) -> Result<(), &'static str> {
        if !self.enabled {
            return Ok(());
        }

        validate_allocation(self.arena_id, ptr)
    }

    pub fn validate_arena(&self) -> Result<(), String> {
        validate_arena(self.arena_id)
    }

    pub fn leak_report(&self) -> Vec<String> {
        leak_report(self.arena_id)
    }

    pub fn rewind(&self, checkpoint_id: usize) {
        if self.enabled {
            rewind_to_checkpoint(checkpoint_id);
        }
    }
}

impl Drop for DebugAllocator {
    fn drop(&mut self) {
        cleanup_arena(self.arena_id);
    }
}

impl Default for DebugAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// Memory validation utilities
pub fn validate_all_allocations(arena_id: usize) -> Result<(), String> {
    if let Ok(state) = DEBUG_STATE.read() {
        if let Some(allocations) = state.allocations.get(&arena_id) {
            let mut errors = Vec::new();

            for info in allocations {
                if let Err(e) = state.validate_allocation(arena_id, info.ptr) {
                    errors.push(format!("Pointer {:p}: {}", info.ptr, e));
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            }
        } else {
            Ok(())
        }
    } else {
        Err("Debug state locked".to_string())
    }
}

// Check for memory corruption
pub fn check_memory_corruption(arena_id: usize) -> usize {
    if let Ok(state) = DEBUG_STATE.read() {
        if let Some(allocations) = state.allocations.get(&arena_id) {
            let mut corrupted = 0;

            for info in allocations {
                if state.validate_allocation(arena_id, info.ptr).is_err() {
                    corrupted += 1;
                }
            }

            corrupted
        } else {
            0
        }
    } else {
        0
    }
}
