//! Debug information for mapping bytecode to source code.

// Debug info structures are defined but not fully used yet
#![allow(dead_code)]

/// A line table entry mapping a bytecode offset to a source location.
#[derive(Debug, Clone)]
pub struct LineEntry {
    /// Bytecode offset (program counter)
    pub pc: u32,
    /// Source file index
    pub file_id: u16,
    /// Line number (1-based)
    pub line: u32,
    /// Column number (1-based)
    pub column: u16,
}

/// A line table for mapping bytecode offsets to source locations.
#[derive(Debug, Clone, Default)]
pub struct LineTable {
    pub entries: Vec<LineEntry>,
}

impl LineTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry to the line table.
    pub fn add(&mut self, pc: usize, line: usize, column: usize) {
        self.entries.push(LineEntry {
            pc: pc as u32,
            file_id: 0, // Single file for now
            line: line as u32,
            column: column as u16,
        });
    }

    /// Find the source location for a given bytecode offset.
    /// Returns (line, column) or None if not found.
    pub fn find_location(&self, pc: usize) -> Option<(u32, u16)> {
        let pc = pc as u32;
        // Find the entry with the largest pc <= target pc
        let mut best: Option<&LineEntry> = None;
        for entry in &self.entries {
            if entry.pc <= pc {
                match best {
                    Some(b) if entry.pc > b.pc => best = Some(entry),
                    None => best = Some(entry),
                    _ => {}
                }
            }
        }
        best.map(|e| (e.line, e.column))
    }
}

/// Local variable debug information.
#[derive(Debug, Clone)]
pub struct LocalVarInfo {
    /// Variable name
    pub name: String,
    /// Slot index in the locals array
    pub slot: u16,
    /// PC where the variable becomes valid
    pub scope_start: u32,
    /// PC where the variable becomes invalid
    pub scope_end: u32,
}

/// Debug info for a function.
#[derive(Debug, Clone, Default)]
pub struct FunctionDebugInfo {
    /// Line table for this function
    pub lines: LineTable,
    /// Local variable information
    pub locals: Vec<LocalVarInfo>,
}

impl FunctionDebugInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a local variable.
    pub fn add_local(&mut self, name: String, slot: usize, scope_start: usize) {
        self.locals.push(LocalVarInfo {
            name,
            slot: slot as u16,
            scope_start: scope_start as u32,
            scope_end: u32::MAX,
        });
    }

    /// Close the scope for a local variable.
    pub fn close_local(&mut self, slot: usize, scope_end: usize) {
        for local in self.locals.iter_mut().rev() {
            if local.slot == slot as u16 && local.scope_end == u32::MAX {
                local.scope_end = scope_end as u32;
                return;
            }
        }
    }

    /// Get locals valid at a given PC.
    pub fn get_locals_at(&self, pc: usize) -> Vec<&LocalVarInfo> {
        let pc = pc as u32;
        self.locals
            .iter()
            .filter(|l| l.scope_start <= pc && pc < l.scope_end)
            .collect()
    }
}

/// Complete debug info for a compiled chunk.
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// Source file names
    pub files: Vec<String>,
    /// Debug info for each function (indexed same as Chunk::functions)
    pub functions: Vec<FunctionDebugInfo>,
    /// Debug info for main code
    pub main: FunctionDebugInfo,
}

impl DebugInfo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_table() {
        let mut table = LineTable::new();
        table.add(0, 1, 1);
        table.add(5, 2, 5);
        table.add(10, 3, 1);

        assert_eq!(table.find_location(0), Some((1, 1)));
        assert_eq!(table.find_location(3), Some((1, 1)));
        assert_eq!(table.find_location(5), Some((2, 5)));
        assert_eq!(table.find_location(7), Some((2, 5)));
        assert_eq!(table.find_location(10), Some((3, 1)));
        assert_eq!(table.find_location(100), Some((3, 1)));
    }

    #[test]
    fn test_local_var_info() {
        let mut info = FunctionDebugInfo::new();
        info.add_local("x".to_string(), 0, 0);
        info.add_local("y".to_string(), 1, 5);
        info.close_local(0, 10);
        info.close_local(1, 15);

        let locals = info.get_locals_at(3);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].name, "x");

        let locals = info.get_locals_at(7);
        assert_eq!(locals.len(), 2);
    }
}
