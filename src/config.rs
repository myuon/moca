//! Runtime configuration types.

use std::time::Duration;

/// Format for timing output
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimingsFormat {
    /// Human-readable table format
    #[default]
    Human,
    /// JSON format for machine consumption
    Json,
}

/// Compiler pipeline timings
#[derive(Debug, Clone, Default)]
pub struct CompilerTimings {
    pub import: Duration,
    pub lexer: Duration,
    pub parser: Duration,
    pub typecheck: Duration,
    pub desugar: Duration,
    pub monomorphise: Duration,
    pub resolve: Duration,
    pub codegen: Duration,
    pub execution: Duration,
}

impl CompilerTimings {
    /// Calculate total time across all phases
    pub fn total(&self) -> Duration {
        self.import
            + self.lexer
            + self.parser
            + self.typecheck
            + self.desugar
            + self.monomorphise
            + self.resolve
            + self.codegen
            + self.execution
    }

    /// Format a duration for display (auto-switch between s and ms)
    fn format_duration(d: Duration) -> String {
        let secs = d.as_secs_f64();
        if secs >= 1.0 {
            format!("{:.2}s", secs)
        } else {
            format!("{:.2}ms", secs * 1000.0)
        }
    }

    /// Output timings in human-readable table format to stderr
    pub fn print_human(&self) {
        eprintln!("=== Compiler Timings ===");
        eprintln!("import:        {:>10}", Self::format_duration(self.import));
        eprintln!("lexer:         {:>10}", Self::format_duration(self.lexer));
        eprintln!("parser:        {:>10}", Self::format_duration(self.parser));
        eprintln!(
            "typecheck:     {:>10}",
            Self::format_duration(self.typecheck)
        );
        eprintln!("desugar:       {:>10}", Self::format_duration(self.desugar));
        eprintln!(
            "monomorphise:  {:>10}",
            Self::format_duration(self.monomorphise)
        );
        eprintln!("resolve:       {:>10}", Self::format_duration(self.resolve));
        eprintln!("codegen:       {:>10}", Self::format_duration(self.codegen));
        eprintln!(
            "execution:     {:>10}",
            Self::format_duration(self.execution)
        );
        eprintln!("------------------------");
        eprintln!("total:         {:>10}", Self::format_duration(self.total()));
    }

    /// Output timings in JSON format to stderr
    pub fn print_json(&self) {
        let json = format!(
            r#"{{"import_ms":{:.2},"lexer_ms":{:.2},"parser_ms":{:.2},"typecheck_ms":{:.2},"desugar_ms":{:.2},"monomorphise_ms":{:.2},"resolve_ms":{:.2},"codegen_ms":{:.2},"execution_ms":{:.2},"total_ms":{:.2}}}"#,
            self.import.as_secs_f64() * 1000.0,
            self.lexer.as_secs_f64() * 1000.0,
            self.parser.as_secs_f64() * 1000.0,
            self.typecheck.as_secs_f64() * 1000.0,
            self.desugar.as_secs_f64() * 1000.0,
            self.monomorphise.as_secs_f64() * 1000.0,
            self.resolve.as_secs_f64() * 1000.0,
            self.codegen.as_secs_f64() * 1000.0,
            self.execution.as_secs_f64() * 1000.0,
            self.total().as_secs_f64() * 1000.0,
        );
        eprintln!("{}", json);
    }

    /// Print timings based on format
    pub fn print(&self, format: TimingsFormat) {
        match format {
            TimingsFormat::Human => self.print_human(),
            TimingsFormat::Json => self.print_json(),
        }
    }
}

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
    /// Whether to profile opcode execution counts
    pub profile_opcodes: bool,
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
            profile_opcodes: false,
        }
    }
}
