---
title: Struct Declarations Specification
description: 構造体型の仕様。固定フィールドを持つデータ型、impl ブロックによるメソッド定義、名義的型付けを定義。
---

# Struct Declarations Specification

## Overview

Add struct declarations to moca, providing a structured data type with fixed fields. Unlike dynamic objects, structs have a known set of fields at compile time, enabling optimizations where property names can be discarded and values represented as data tuples.

## Syntax

### Struct Declaration

```mc
struct Point {
    x: int,
    y: int,
}

struct Person {
    name: string,
    age: int,
    email: string?,
}
```

### Instance Creation

Object literal style with struct name prefix:

```mc
let p = Point { x: 10, y: 20 };
let person = Person { name: "Alice", age: 30, email: nil };
```

All fields must be provided (no default values).

### Field Access

```mc
let x = p.x;
let name = person.name;
```

### Field Assignment

All fields are mutable:

```mc
p.x = 100;
person.age = 31;
```

### Methods (impl blocks)

```mc
struct Rectangle {
    width: int,
    height: int,
}

impl Rectangle {
    fn area(self) -> int {
        return self.width * self.height;
    }

    fn scale(self, factor: int) {
        self.width = self.width * factor;
        self.height = self.height * factor;
    }
}

let rect = Rectangle { width: 10, height: 5 };
let a = rect.area();  // 50
rect.scale(2);
```

## Type System Integration

### Struct Types

Structs introduce a new named type:

```mc
let p: Point = Point { x: 1, y: 2 };
```

Struct types are nominal (not structural). Two structs with identical fields are different types:

```mc
struct Vec2 { x: int, y: int }
struct Point { x: int, y: int }

let v: Vec2 = Vec2 { x: 1, y: 2 };
let p: Point = v;  // ERROR: type mismatch
```

### Nullable Fields

Fields can have nullable types:

```mc
struct Node {
    value: int,
    next: Node?,  // Forward reference allowed
}
```

### Field Order

Fields are stored in declaration order. This enables tuple-like value representation internally.

## Comparison

Struct comparison (`==`, `!=`) is **not allowed**. Users must implement their own comparison logic:

```mc
let p1 = Point { x: 1, y: 2 };
let p2 = Point { x: 1, y: 2 };

// p1 == p2  // ERROR: cannot compare struct values
```

## Value Representation

Since struct fields are fixed and ordered, the runtime can optimize storage:

1. Property names are resolved at compile time to field indices
2. Values are stored as ordered tuples (no hash map overhead)
3. Field access becomes index-based (O(1))

Example internal representation:
```
Point { x: 10, y: 20 }  -->  [10, 20]  (with type tag for Point)
```

## Grammar Changes

```
item ::= ... | struct_decl | impl_block

struct_decl ::= 'struct' IDENT '{' struct_fields '}'
struct_fields ::= (struct_field ',')* struct_field?
struct_field ::= IDENT ':' type

impl_block ::= 'impl' IDENT '{' fn_def* '}'

primary_expr ::= ... | struct_literal
struct_literal ::= IDENT '{' struct_init_fields '}'
struct_init_fields ::= (struct_init_field ',')* struct_init_field?
struct_init_field ::= IDENT ':' expr
```

## New Keywords

- `struct` - declares a struct type
- `impl` - declares an implementation block for a struct

## Acceptance Criteria

### Parsing
1. `struct Point { x: int, y: int }` parses successfully
2. `impl Point { fn origin() -> Point { ... } }` parses successfully
3. `Point { x: 1, y: 2 }` parses as struct literal expression

### Type Checking
4. `let p: Point = Point { x: 1, y: 2 };` type checks
5. `let p: Point = { x: 1, y: 2 };` is a type error (plain object vs struct)
6. `Point { x: 1 }` is a type error (missing field `y`)
7. `Point { x: 1, y: 2, z: 3 }` is a type error (extra field `z`)
8. `Point { x: "a", y: 2 }` is a type error (wrong field type)
9. Method calls on struct values type check correctly

### Runtime
10. Field access returns correct values
11. Field assignment modifies the struct
12. Method calls work with `self` parameter

### Errors
13. `p1 == p2` produces a type error for struct values
14. Adding new fields to struct instance produces error

## Non-Goals (Future Work)

- Struct inheritance
- Generic structs (`struct Pair<T> { ... }`)
- Derive macros for common traits
- Pattern matching on structs
- Associated constants
- Static methods without `self`

## Implementation Notes

### Compiler Changes

1. **Lexer**: Add `struct` and `impl` keywords
2. **Parser**: Parse struct declarations, impl blocks, struct literals
3. **AST**: Add `StructDef`, `ImplBlock`, `StructLiteral` nodes
4. **Type System**: Add `Type::Struct { name, fields }` variant
5. **Type Checker**:
   - Register struct types
   - Validate struct literals
   - Type check impl methods with `self`
6. **Resolver**: Resolve struct field accesses to indices
7. **Codegen**:
   - Generate struct type metadata
   - Emit tuple-based value representation
   - Compile field access as index operations

### VM Changes

1. Add `Value::Struct` variant (or reuse object with type tag)
2. Optimize field access to use indices instead of string lookup
