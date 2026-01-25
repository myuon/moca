---
title: Debugger Specification
description: TUI デバッガーの仕様。ブレークポイント、ステップ実行、変数検査、コールスタック確認機能を提供。
---

# Mica Debugger Specification

This document defines the TUI debugger for the Mica language.

## Overview

The Mica debugger provides:
- Breakpoint management
- Step execution
- Variable inspection
- Call stack examination

## Starting the Debugger

```bash
mica debug [file]
```

## Commands

| Function | Command |
|----------|---------|
| Set breakpoint | `b <file>:<line>` or `b <func>` |
| Delete breakpoint | `d <id>` |
| List breakpoints | `bl` |
| Continue | `c` (continue) |
| Step into | `s` (step) |
| Step over | `n` (next) |
| Step out | `finish` |
| Backtrace | `bt` (backtrace) |
| Print expression | `p <expr>` (print) |
| List locals | `locals` |
| Add watch | `w <expr>` (watch) |
| Quit | `q` (quit) |

## TUI Layout

```
┌─────────────────────────────────────────────────────────┐
│ src/main.mica                                           │
├─────────────────────────────────────────────────────────┤
│   8 │ fn main() {                                       │
│   9 │     let x = 10;                                   │
│ ▶10 │     let y = compute(x);  // ← Current position    │
│  11 │     print(y);                                     │
│  12 │ }                                                 │
├─────────────────────────────────────────────────────────┤
│ Locals              │ Call Stack                        │
│ x = 10              │ #0 main (main.mica:10)            │
│ y = <uninitialized> │                                   │
├─────────────────────────────────────────────────────────┤
│ (debug) _                                               │
└─────────────────────────────────────────────────────────┘
```

### Panels

1. **Source Panel** (top): Displays source code with current execution position marked
2. **Locals Panel** (bottom-left): Shows local variables and their values
3. **Call Stack Panel** (bottom-right): Shows the call stack
4. **Command Panel** (bottom): Input area for debug commands

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `F5` | Continue |
| `F10` | Step over |
| `F11` | Step into |
| `Shift+F11` | Step out |
| `F9` | Toggle breakpoint at cursor |
| `Ctrl+C` | Interrupt execution |
| `Ctrl+Q` | Quit debugger |

## Debug Information

### Source Map (PC → Source Position)

```rust
struct LineTable {
    entries: Vec<LineEntry>,
}

struct LineEntry {
    pc: u32,           // Bytecode offset
    file_id: u16,      // File identifier
    line: u32,         // Line number (1-based)
    column: u16,       // Column number (1-based)
}
```

### Local Variable Metadata

```rust
struct LocalVarInfo {
    name: String,
    slot: u16,         // Index in locals[]
    scope_start: u32,  // Scope start PC
    scope_end: u32,    // Scope end PC
    type_hint: String, // Type information (inference result)
}
```

## Example Session

```
$ mica debug example.mica

Loading example.mica...
(debug) b add
Breakpoint 1 set at add (example.mica:1)

(debug) c
Running...
Hit breakpoint 1 at add (example.mica:1)

(debug) p a
a = 3

(debug) p b
b = 4

(debug) locals
a = 3
b = 4

(debug) bt
#0  add (example.mica:2)
#1  main (example.mica:6)

(debug) n
Stepped to example.mica:2

(debug) c
Output: 7
Program finished.

(debug) q
```

## Watch Expressions

Watch expressions are evaluated at each step:

```
(debug) w a + b
Watch 1: a + b

(debug) s
Watch 1: a + b = 7
```

## Conditional Breakpoints

Set breakpoints that only trigger when a condition is true:

```
(debug) b example.mica:10 if i == 5
Conditional breakpoint 2 set at example.mica:10 (when i == 5)
```

## Expression Evaluation

The `p` command evaluates expressions in the current context:

```
(debug) p x * 2 + 1
21

(debug) p arr[0]
1

(debug) p obj.field
"value"
```

Note: Expression evaluation should not have side effects. Assignments and function calls with side effects are not allowed in debug expressions.
