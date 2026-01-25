//! Concurrent garbage collection support.
//!
//! This module implements a concurrent mark-sweep GC with write barriers.
//! The GC uses a snapshot-at-the-beginning (SATB) write barrier to ensure
//! correctness during concurrent marking.

// Concurrent GC is not yet integrated, allow dead code
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::heap::GcRef;
use super::Value;

/// GC phase states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcPhase {
    /// No GC in progress
    Idle,
    /// Initial mark phase (STW)
    InitialMark,
    /// Concurrent marking phase
    ConcurrentMark,
    /// Remark phase (STW)
    Remark,
    /// Concurrent sweep phase
    ConcurrentSweep,
}

/// Statistics for GC operations.
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total number of GC cycles
    pub cycles: usize,
    /// Total time spent in initial mark (microseconds)
    pub initial_mark_us: u64,
    /// Total time spent in concurrent mark (microseconds)
    pub concurrent_mark_us: u64,
    /// Total time spent in remark (microseconds)
    pub remark_us: u64,
    /// Total time spent in concurrent sweep (microseconds)
    pub sweep_us: u64,
    /// Maximum pause time (microseconds)
    pub max_pause_us: u64,
    /// Total objects marked
    pub objects_marked: usize,
    /// Total objects swept
    pub objects_swept: usize,
}

/// Concurrent GC state.
pub struct ConcurrentGc {
    /// Current GC phase
    phase: GcPhase,
    /// Whether marking is in progress (for write barrier)
    marking: AtomicBool,
    /// Gray worklist for concurrent marking
    gray_list: Mutex<VecDeque<GcRef>>,
    /// Saturation worklist for SATB barrier
    satb_buffer: Mutex<Vec<GcRef>>,
    /// GC statistics
    stats: GcStats,
    /// Whether concurrent GC is enabled
    enabled: bool,
}

impl ConcurrentGc {
    /// Create a new concurrent GC instance.
    pub fn new(enabled: bool) -> Self {
        Self {
            phase: GcPhase::Idle,
            marking: AtomicBool::new(false),
            gray_list: Mutex::new(VecDeque::new()),
            satb_buffer: Mutex::new(Vec::new()),
            stats: GcStats::default(),
            enabled,
        }
    }

    /// Check if concurrent GC is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the current GC phase.
    pub fn phase(&self) -> GcPhase {
        self.phase
    }

    /// Check if marking is in progress (for write barrier).
    pub fn is_marking(&self) -> bool {
        self.marking.load(Ordering::Acquire)
    }

    /// Get GC statistics.
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }

    /// SATB write barrier - call before overwriting a reference.
    ///
    /// This implements the snapshot-at-the-beginning barrier:
    /// When a reference is about to be overwritten, the old value
    /// is recorded so it won't be lost during concurrent marking.
    pub fn write_barrier(&self, old_value: Value) {
        if !self.is_marking() {
            return;
        }

        // Only care about pointer values
        if let Some(gc_ref) = old_value.as_ptr() {
            // Add to SATB buffer for later processing
            if let Ok(mut buffer) = self.satb_buffer.lock() {
                buffer.push(gc_ref);
            }
        }
    }

    /// Mark an object as gray (reachable but not yet scanned).
    pub fn mark_gray(&self, gc_ref: GcRef) {
        if let Ok(mut gray_list) = self.gray_list.lock() {
            gray_list.push_back(gc_ref);
        }
    }

    /// Start the initial mark phase.
    /// Returns the roots that were marked.
    pub fn start_initial_mark(&mut self, roots: &[Value]) -> Vec<GcRef> {
        self.phase = GcPhase::InitialMark;
        self.marking.store(true, Ordering::Release);

        let start = std::time::Instant::now();

        // Collect root references
        let root_refs: Vec<GcRef> = roots.iter().filter_map(|v| v.as_ptr()).collect();

        // Add roots to gray list
        {
            let mut gray_list = self.gray_list.lock().unwrap();
            gray_list.extend(root_refs.iter().copied());
        }

        self.stats.initial_mark_us += start.elapsed().as_micros() as u64;
        self.stats.max_pause_us = self.stats.max_pause_us.max(start.elapsed().as_micros() as u64);

        self.phase = GcPhase::ConcurrentMark;
        root_refs
    }

    /// Process a batch of gray objects during concurrent marking.
    /// Returns true if there's more work to do.
    pub fn mark_step<F>(&mut self, mut mark_fn: F, batch_size: usize) -> bool
    where
        F: FnMut(GcRef) -> Vec<GcRef>,
    {
        let start = std::time::Instant::now();
        let mut processed = 0;

        while processed < batch_size {
            let gc_ref = {
                let mut gray_list = self.gray_list.lock().unwrap();
                gray_list.pop_front()
            };

            match gc_ref {
                Some(r) => {
                    // Mark the object and get its children
                    let children = mark_fn(r);
                    self.stats.objects_marked += 1;

                    // Add children to gray list
                    {
                        let mut gray_list = self.gray_list.lock().unwrap();
                        gray_list.extend(children);
                    }

                    processed += 1;
                }
                None => break,
            }
        }

        self.stats.concurrent_mark_us += start.elapsed().as_micros() as u64;

        // Check if there's more work
        

        {
            let gray_list = self.gray_list.lock().unwrap();
            !gray_list.is_empty()
        }
    }

    /// Process SATB buffer entries during remark.
    pub fn process_satb_buffer<F>(&mut self, mut mark_fn: F)
    where
        F: FnMut(GcRef) -> Vec<GcRef>,
    {
        let entries: Vec<GcRef> = {
            let mut buffer = self.satb_buffer.lock().unwrap();
            std::mem::take(&mut *buffer)
        };

        // Add all SATB entries to gray list
        {
            let mut gray_list = self.gray_list.lock().unwrap();
            gray_list.extend(entries);
        }

        // Process all remaining gray objects
        while self.mark_step(&mut mark_fn, 1000) {}
    }

    /// Start the remark phase.
    pub fn start_remark<F>(&mut self, mark_fn: F)
    where
        F: FnMut(GcRef) -> Vec<GcRef>,
    {
        self.phase = GcPhase::Remark;

        let start = std::time::Instant::now();

        // Process any remaining SATB buffer entries
        self.process_satb_buffer(mark_fn);

        self.stats.remark_us += start.elapsed().as_micros() as u64;
        self.stats.max_pause_us = self.stats.max_pause_us.max(start.elapsed().as_micros() as u64);

        self.marking.store(false, Ordering::Release);
        self.phase = GcPhase::ConcurrentSweep;
    }

    /// Complete the GC cycle.
    pub fn complete(&mut self, objects_swept: usize) {
        self.stats.objects_swept += objects_swept;
        self.stats.cycles += 1;
        self.phase = GcPhase::Idle;
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.stats = GcStats::default();
    }
}

impl Default for ConcurrentGc {
    fn default() -> Self {
        Self::new(true)
    }
}

/// Write barrier helper for VM operations.
/// Call this before writing to a heap object field.
pub fn write_barrier(gc: &ConcurrentGc, old_value: Value) {
    gc.write_barrier(old_value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_phases() {
        let gc = ConcurrentGc::new(true);
        assert_eq!(gc.phase(), GcPhase::Idle);
        assert!(!gc.is_marking());
    }

    #[test]
    fn test_initial_mark() {
        let mut gc = ConcurrentGc::new(true);

        let roots = vec![
            Value::Int(42),
            Value::Ptr(GcRef { index: 0 }),
            Value::Ptr(GcRef { index: 1 }),
        ];

        let marked = gc.start_initial_mark(&roots);
        assert_eq!(marked.len(), 2); // Only pointer values
        assert!(gc.is_marking());
        assert_eq!(gc.phase(), GcPhase::ConcurrentMark);
    }

    #[test]
    fn test_write_barrier_during_marking() {
        let mut gc = ConcurrentGc::new(true);

        // Before marking, barrier should do nothing
        gc.write_barrier(Value::Ptr(GcRef { index: 0 }));
        {
            let buffer = gc.satb_buffer.lock().unwrap();
            assert!(buffer.is_empty());
        }

        // Start marking
        gc.start_initial_mark(&[]);

        // Now barrier should record old values
        gc.write_barrier(Value::Ptr(GcRef { index: 5 }));
        gc.write_barrier(Value::Int(42)); // Non-pointer, ignored
        gc.write_barrier(Value::Ptr(GcRef { index: 10 }));

        {
            let buffer = gc.satb_buffer.lock().unwrap();
            assert_eq!(buffer.len(), 2);
            assert_eq!(buffer[0].index, 5);
            assert_eq!(buffer[1].index, 10);
        }
    }

    #[test]
    fn test_mark_step() {
        let mut gc = ConcurrentGc::new(true);

        // Add some gray objects
        gc.mark_gray(GcRef { index: 0 });
        gc.mark_gray(GcRef { index: 1 });
        gc.mark_gray(GcRef { index: 2 });

        // Process with a mock mark function
        let mut marked = Vec::new();
        let has_more = gc.mark_step(|r| {
            marked.push(r);
            vec![] // No children
        }, 2);

        assert_eq!(marked.len(), 2);
        assert!(has_more); // One more object remaining

        let has_more = gc.mark_step(|r| {
            marked.push(r);
            vec![]
        }, 10);

        assert_eq!(marked.len(), 3);
        assert!(!has_more); // No more work
    }

    #[test]
    fn test_full_gc_cycle() {
        let mut gc = ConcurrentGc::new(true);

        // Initial mark
        gc.start_initial_mark(&[Value::Ptr(GcRef { index: 0 })]);
        assert_eq!(gc.phase(), GcPhase::ConcurrentMark);

        // Concurrent mark
        while gc.mark_step(|_| vec![], 10) {}

        // Remark
        gc.start_remark(|_| vec![]);
        assert_eq!(gc.phase(), GcPhase::ConcurrentSweep);
        assert!(!gc.is_marking());

        // Complete
        gc.complete(5);
        assert_eq!(gc.phase(), GcPhase::Idle);
        assert_eq!(gc.stats().cycles, 1);
        assert_eq!(gc.stats().objects_swept, 5);
    }
}
