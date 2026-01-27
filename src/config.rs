//! Runtime configuration types.

/// JIT compilation mode
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JitMode {
    /// JIT disabled, interpreter only
    Off,
    /// JIT enabled (compile hot functions)
    On,
    /// Automatic: JIT enabled if supported on this platform
    #[default]
    Auto,
}

/// GC mode
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GcMode {
    /// Stop-the-world GC
    #[default]
    Stw,
    /// Concurrent GC (reduced pause times)
    Concurrent,
}

/// Runtime configuration for the VM
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub jit_mode: JitMode,
    pub jit_threshold: u32,
    pub trace_jit: bool,
    #[allow(dead_code)] // Reserved for future GC configuration
    pub gc_mode: GcMode,
    pub gc_stats: bool,
    /// Whether GC is enabled (default: true)
    pub gc_enabled: bool,
    /// Hard limit on heap size in bytes (None = unlimited)
    pub heap_limit: Option<usize>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            jit_mode: JitMode::Auto,
            jit_threshold: 1000,
            trace_jit: false,
            gc_mode: GcMode::Stw,
            gc_stats: false,
            gc_enabled: true,
            heap_limit: None,
        }
    }
}
