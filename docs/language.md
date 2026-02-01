---
title: Language Specification
description: Moca プログラミング言語の構文とセマンティクス。型システム、制御フロー、並行処理、例外処理を定義。
---

# Moca Language Specification

This document defines the syntax and semantics of the Moca programming language.

## Types

### Primitive Types

| Type | Representation | Notes |
|------|----------------|-------|
| `int` | SMI (63-bit) / boxed i64 | Signed integer |
| `float` | boxed f64 | IEEE 754 double precision |
| `bool` | Tag value | `true` / `false` |
| `nil` | Tag value | Equivalent to null |
| `any` | Any value | Bypasses type checking, unifies with any type |

### Compound Types

| Type | Representation | Notes |
|------|----------------|-------|
| `string` | Heap object | UTF-8, immutable |
| `array` | Heap object | Fixed-length array of Values |
| `Vector` | Heap object | Dynamic array with ptr/len/cap layout |
| `HashMapAny` | Heap object | Key → Value mapping (stdlib) |

## Syntax

### Comments

```
// Line comment (from // to end of line)
```

### Variable Declaration

```
// Immutable variable
let x = 42;

// Mutable variable
var y = 0;

// Assignment (var only)
y = y + 1;
```

### Functions

```
// Function definition
fun add(a, b) {
    return a + b;
}

// Function call
let result = add(1, 2);

// Functions can be called before definition (hoisting)
```

### Control Flow

```
// if-else
if x > 0 {
    print(x);
} else {
    print(0);
}

// while loop
while y < 10 {
    print(y);
    y = y + 1;
}

// for-in loop (array iteration)
for item in arr {
    print(item);
}
```

### Literals

```
// Integer literals
let a = 42;
let b = -1;
let c = 0;

// Boolean literals
let t = true;
let f = false;

// String literals
let s = "hello, world";
let escaped = "line1\nline2";

// Array literals
var arr = [1, 2, 3];
let first = arr[0];
arr[1] = 42;

// Map (use stdlib HashMap functions)
let m = map_new_any();
map_put_string(m, "x", 10);
map_put_string(m, "y", 20);
let x = map_get_string(m, "x");

// nil
let nothing = nil;
```

### Vector Operations

```
// Create a new vector
var vec = vec_new();

// Push elements
vec_push(vec, 10);
vec_push(vec, 20);
vec_push(vec, 30);

// Access elements using index syntax (same as array)
let first = vec[0];    // Read: returns 10
vec[1] = 25;           // Write: sets vec[1] to 25

// Get length and capacity
let length = vec_len(vec);       // Current length
let capacity = vec_capacity(vec); // Current capacity

// Pop element
let last = vec_pop(vec);  // Removes and returns last element
```

**Note:** Vector index access (`vec[i]`) uses the type system to differentiate from array access. The compiler generates different bytecode:
- **Vector**: `HeapLoad(0)` (get data ptr) → `HeapLoadDyn`/`HeapStoreDyn`
- **Array**: Direct `HeapLoadDyn`/`HeapStoreDyn`

### Exception Handling

```
// Throw an exception
throw "error message";

// Try-catch
try {
    risky_operation();
} catch e {
    print(e);
}
```

### Concurrency

```
// Spawn a thread
let handle = spawn(fn() {
    heavy_computation();
});

// Wait for result
let result = handle.join();

// Channel communication
let (tx, rx) = channel();
spawn(fn() {
    tx.send(42);
});
let value = rx.recv();
```

## Tokens

| Category | Tokens |
|----------|--------|
| Keywords | `let`, `var`, `fun`, `if`, `else`, `while`, `for`, `in`, `return`, `true`, `false`, `nil`, `try`, `catch`, `throw`, `spawn` |
| Literals | Integer (`0`, `42`, `-1`), Float (`3.14`), Bool (`true`, `false`), String (`"hello"`) |
| Identifiers | `[a-zA-Z_][a-zA-Z0-9_]*` |
| Operators | `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `\|\|`, `!` |
| Delimiters | `(`, `)`, `{`, `}`, `[`, `]`, `,`, `;`, `=`, `.`, `:` |
| Comments | `//` to end of line |

## Operator Precedence (Low → High)

1. `||`
2. `&&`
3. `==`, `!=`
4. `<`, `<=`, `>`, `>=`
5. `+`, `-`
6. `*`, `/`, `%`
7. `!`, `-` (unary)

## Grammar (EBNF)

```ebnf
program     = { item } ;
item        = fn_def | statement ;

fn_def      = "fun" IDENT "(" [ params ] ")" block ;
params      = IDENT { "," IDENT } ;

block       = "{" { statement } "}" ;

statement   = let_stmt
            | var_stmt
            | assign_stmt
            | if_stmt
            | while_stmt
            | for_stmt
            | return_stmt
            | try_stmt
            | throw_stmt
            | expr_stmt ;

let_stmt    = "let" IDENT "=" expr ";" ;
var_stmt    = "var" IDENT "=" expr ";" ;
assign_stmt = IDENT "=" expr ";"
            | IDENT "[" expr "]" "=" expr ";"
            | IDENT "." IDENT "=" expr ";" ;
if_stmt     = "if" expr block [ "else" block ] ;
while_stmt  = "while" expr block ;
for_stmt    = "for" IDENT "in" expr block ;
return_stmt = "return" [ expr ] ";" ;
try_stmt    = "try" block "catch" IDENT block ;
throw_stmt  = "throw" expr ";" ;
expr_stmt   = expr ";" ;

expr        = or_expr ;
or_expr     = and_expr { "||" and_expr } ;
and_expr    = eq_expr { "&&" eq_expr } ;
eq_expr     = cmp_expr { ( "==" | "!=" ) cmp_expr } ;
cmp_expr    = add_expr { ( "<" | "<=" | ">" | ">=" ) add_expr } ;
add_expr    = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr    = unary_expr { ( "*" | "/" | "%" ) unary_expr } ;
unary_expr  = ( "!" | "-" ) unary_expr | call_expr ;
call_expr   = primary { "(" [ args ] ")" | "[" expr "]" | "." IDENT } ;
args        = expr { "," expr } ;
primary     = INT | FLOAT | STRING | "true" | "false" | "nil" | IDENT
            | "(" expr ")"
            | "[" [ args ] "]" ;
```

## Semantics

- Integers are 63-bit signed (embedded in 64-bit Value as SMI)
- Bool: `true` = 1, `false` = 0 for arithmetic
- Division by zero is a runtime error
- Undefined variable reference is a compile error
- Functions can be called before definition (hoisting)
- `print` is a built-in function (outputs value to stdout)
- Top-level statements execute sequentially as implicit `main`

## Built-in Functions

| Function | Description |
|----------|-------------|
| `print(v)` | Output value to stdout |
| `len(arr)` | Get array length |
| `push(arr, v)` | Append element to array |
| `pop(arr)` | Remove and return last element |
| `type_of(v)` | Return type name as string |
| `to_string(v)` | Convert value to string |
| `parse_int(s)` | Parse string to integer |
| `spawn(fn)` | Spawn a new thread |
| `channel()` | Create a channel pair (tx, rx) |

### Vector Functions

| Function | Description |
|----------|-------------|
| `vec_new()` | Create an empty vector |
| `vec_with_capacity(n)` | Create a vector with initial capacity |
| `vec_push(vec, v)` | Append element to vector |
| `vec_pop(vec)` | Remove and return last element |
| `vec_len(vec)` | Get current length |
| `vec_capacity(vec)` | Get current capacity |
| `vec_get(vec, i)` | Get element at index (alternative to `vec[i]`) |
| `vec_set(vec, i, v)` | Set element at index (alternative to `vec[i] = v`) |

### HashMap Functions

Functions for key-value storage using the stdlib HashMap implementation.

| Function | Description |
|----------|-------------|
| `map_new_any()` | Create an empty HashMap |
| `map_put_string(m, key, val)` | Insert with string key |
| `map_get_string(m, key)` | Get value by string key |
| `map_has_string(m, key)` | Check if string key exists |
| `map_remove_string(m, key)` | Remove entry by string key |
| `map_put_int(m, key, val)` | Insert with int key |
| `map_get_int(m, key)` | Get value by int key |
| `map_has_int(m, key)` | Check if int key exists |
| `map_remove_int(m, key)` | Remove entry by int key |
| `map_size(m)` | Get number of entries |
| `map_keys(m)` | Get array of all keys |
| `map_values(m)` | Get array of all values |
| `map_clear(m)` | Remove all entries |

**Note:** HashMap value types are inferred from the first `map_put_*` call. Use separate maps for different value types.

### Network Functions

TCP socket operations for client and server networking.

| Function | Description |
|----------|-------------|
| `socket(domain, type)` | Create a socket (use `AF_INET()`, `SOCK_STREAM()`) |
| `connect(fd, host, port)` | Connect to a remote host |
| `bind(fd, host, port)` | Bind socket to a local address |
| `listen(fd, backlog)` | Start listening for connections |
| `accept(fd)` | Accept an incoming connection, returns new fd |
| `read(fd, count)` | Read up to count bytes from fd |
| `write(fd, buf, count)` | Write buf to fd |
| `close(fd)` | Close a file descriptor |

**Constants:**
- `AF_INET()` - IPv4 address family
- `SOCK_STREAM()` - TCP socket type
- Error codes: `EBADF()`, `ECONNREFUSED()`, `ETIMEDOUT()`, `EADDRINUSE()`, etc.

**Example: HTTP Server**

```
fun main() {
    let fd = socket(AF_INET(), SOCK_STREAM());
    bind(fd, "0.0.0.0", 8080);
    listen(fd, 10);

    while true {
        let client = accept(fd);
        let request = read(client, 4096);
        let response = "HTTP/1.1 200 OK\r\n\r\nHello!";
        write(client, response, len(response));
        close(client);
    }
}
```

## Error Format

```
error: <message>
  --> <file>:<line>:<column>
```

## Sample Programs

### FizzBuzz

```
fun fizzbuzz(n) {
    var i = 1;
    while i <= n {
        if i % 15 == 0 {
            print("FizzBuzz");
        } else if i % 3 == 0 {
            print("Fizz");
        } else if i % 5 == 0 {
            print("Buzz");
        } else {
            print(i);
        }
        i = i + 1;
    }
}

fizzbuzz(15);
```

### Fibonacci

```
fun fib(n) {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

var i = 0;
while i < 10 {
    print(fib(i));
    i = i + 1;
}
```

### Factorial

```
fun fact(n) {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

print(fact(5));  // 120
print(fact(10)); // 3628800
```
