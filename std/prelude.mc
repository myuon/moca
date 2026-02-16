// Moca Standard Library - Prelude
// This file is automatically loaded when running Moca programs.

// ============================================================================
// Syscall Numbers (internal use)
// ============================================================================
// Syscall 1: write(fd, buf, count) -> bytes_written
// Syscall 2: open(path, flags) -> fd
// Syscall 3: close(fd) -> status
// Syscall 4: read(fd, count) -> string
// Syscall 5: socket(domain, type) -> fd
// Syscall 6: connect(fd, host, port) -> status
// Syscall 7: bind(fd, host, port) -> status
// Syscall 8: listen(fd, backlog) -> status
// Syscall 9: accept(fd) -> client_fd
// Syscall 10: time() -> epoch_seconds
// Syscall 11: time_nanos() -> epoch_nanoseconds

// ============================================================================
// POSIX-like Constants (as functions to avoid polluting the stack)
// ============================================================================

// File open flags (Linux-compatible values)
fun O_RDONLY() -> int { return 0; }    // Read only
fun O_WRONLY() -> int { return 1; }    // Write only
fun O_CREAT() -> int { return 64; }    // Create file if not exists
fun O_TRUNC() -> int { return 512; }   // Truncate existing file

// Socket constants (Linux-compatible values)
fun AF_INET() -> int { return 2; }     // IPv4 address family
fun SOCK_STREAM() -> int { return 1; } // TCP socket type

// Error codes (negative return values)
fun EBADF() -> int { return -1; }           // Bad file descriptor
fun ENOENT() -> int { return -2; }          // No such file or directory
fun EACCES() -> int { return -3; }          // Permission denied
fun ECONNREFUSED() -> int { return -4; }    // Connection refused
fun ETIMEDOUT() -> int { return -5; }       // Connection timed out
fun EAFNOSUPPORT() -> int { return -6; }    // Address family not supported
fun ESOCKTNOSUPPORT() -> int { return -7; } // Socket type not supported
fun EADDRINUSE() -> int { return -8; }      // Address already in use

// ============================================================================
// Low-level I/O Functions (using __syscall)
// ============================================================================

// Open a file and return a file descriptor.
// flags: O_RDONLY(), O_WRONLY(), O_CREAT(), O_TRUNC() (can be combined with |)
// Returns: fd (>=3) on success, negative error code on failure
fun open(path: string, flags: int) -> int {
    return __syscall(2, path, flags);
}

// Write to a file descriptor.
// fd: 1 = stdout, 2 = stderr, >=3 = file/socket
// Returns: bytes written on success, negative error code on failure
fun write(fd: int, buf: string, count: int) -> int {
    return __syscall(1, fd, buf, count);
}

// Read from a file descriptor.
// Returns: string on success, or throws on error
fun read(fd: int, count: int) -> string {
    return __syscall(4, fd, count);
}

// Close a file descriptor.
// Returns: 0 on success, negative error code on failure
fun close(fd: int) -> int {
    return __syscall(3, fd);
}

// Create a socket.
// domain: AF_INET() (2) for IPv4
// typ: SOCK_STREAM() (1) for TCP
// Returns: socket fd on success, negative error code on failure
fun socket(domain: int, typ: int) -> int {
    return __syscall(5, domain, typ);
}

// Connect a socket to a remote host.
// Returns: 0 on success, negative error code on failure
fun connect(fd: int, host: string, port: int) -> int {
    return __syscall(6, fd, host, port);
}

// Bind a socket to a local address.
// host: "0.0.0.0" for all interfaces, "127.0.0.1" for localhost only
// Returns: 0 on success, negative error code on failure
fun bind(fd: int, host: string, port: int) -> int {
    return __syscall(7, fd, host, port);
}

// Listen for incoming connections on a bound socket.
// backlog: maximum number of pending connections (ignored in current implementation)
// Returns: 0 on success, negative error code on failure
fun listen(fd: int, backlog: int) -> int {
    return __syscall(8, fd, backlog);
}

// Accept an incoming connection on a listening socket.
// Returns: new socket fd for the client connection, or negative error code on failure
fun accept(fd: int) -> int {
    return __syscall(9, fd);
}

// ============================================================================
// Time Functions
// ============================================================================

// Get current time as Unix epoch seconds.
fun time() -> int {
    return __syscall(10);
}

// Get current time as Unix epoch nanoseconds.
fun time_nanos() -> int {
    return __syscall(11);
}

// ============================================================================
// Value to String Conversion — Helpers
// ============================================================================

// Count decimal digits of an integer (no heap allocation).
@inline
fun _int_digit_count(n: int) -> int {
    if n == 0 {
        return 1;
    }
    let count = 0;
    let val = n;
    if val < 0 {
        count = 1;
        val = -val;
    }
    while val > 0 {
        val = val / 10;
        count = count + 1;
    }
    return count;
}

// Write integer digits into buf at offset, return new offset (no heap allocation).
@inline
fun _int_write_to(buf: any, off: int, n: int) -> int {
    if n == 0 {
        __heap_store(buf, off, 48);
        return off + 1;
    }
    let negative = n < 0;
    let val = n;
    if negative {
        val = -val;
    }
    let dcount = _int_digit_count(n);
    if negative {
        __heap_store(buf, off, 45);
    }
    let pos = off + dcount - 1;
    while val > 0 {
        __heap_store(buf, pos, val % 10 + 48);
        val = val / 10;
        pos = pos - 1;
    }
    return off + dcount;
}

// Copy string data into buf at offset, return new offset.
@inline
fun _str_copy_to(buf: any, off: int, s: string) -> int {
    let ptr = __heap_load(s, 0);
    let slen = __heap_load(s, 1);
    let j = 0;
    while j < slen {
        __heap_store(buf, off + j, __heap_load(ptr, j));
        j = j + 1;
    }
    return off + slen;
}

// Return string length of a bool ("true"=4, "false"=5).
@inline
fun _bool_str_len(b: bool) -> int {
    if b {
        return 4;
    }
    return 5;
}

// Write "true" or "false" into buf at offset, return new offset.
@inline
fun _bool_write_to(buf: any, off: int, b: bool) -> int {
    if b {
        __heap_store(buf, off, 116);
        __heap_store(buf, off + 1, 114);
        __heap_store(buf, off + 2, 117);
        __heap_store(buf, off + 3, 101);
        return off + 4;
    }
    __heap_store(buf, off, 102);
    __heap_store(buf, off + 1, 97);
    __heap_store(buf, off + 2, 108);
    __heap_store(buf, off + 3, 115);
    __heap_store(buf, off + 4, 101);
    return off + 5;
}

// ============================================================================
// Value to String Conversion
// ============================================================================

// Internal: convert integer to string (single heap allocation).
fun _int_to_string(n: int) -> string {
    if n == 0 {
        return "0";
    }
    let dcount = _int_digit_count(n);
    let data = __alloc_heap(dcount);
    _int_write_to(data, 0, n);
    return __alloc_string(data, dcount);
}

// Convert any value to its string representation.
fun to_string(x: any) -> string {
    let t = type_of(x);
    if t == "string" {
        return x;
    }
    if t == "int" {
        return _int_to_string(x);
    }
    if t == "float" {
        return __float_to_string(x);
    }
    if t == "bool" {
        if x {
            return "true";
        }
        return "false";
    }
    return "nil";
}

// Zero-pad an integer to 2 digits.
fun _pad2(n: int) -> string {
    if n < 10 {
        return "0" + to_string(n);
    }
    return to_string(n);
}

// Zero-pad an integer to 4 digits.
fun _pad4(n: int) -> string {
    if n < 10 {
        return "000" + to_string(n);
    }
    if n < 100 {
        return "00" + to_string(n);
    }
    if n < 1000 {
        return "0" + to_string(n);
    }
    return to_string(n);
}

// Check if a year is a leap year.
fun _is_leap_year(y: int) -> bool {
    return y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
}

// Get number of days in a month (1-indexed).
fun _days_in_month(y: int, m: int) -> int {
    if m == 2 {
        if _is_leap_year(y) {
            return 29;
        }
        return 28;
    }
    if m == 4 || m == 6 || m == 9 || m == 11 {
        return 30;
    }
    return 31;
}

// Format epoch seconds as "YYYY-MM-DD HH:MM:SS" (UTC).
// Uses civil_from_days algorithm for date calculation.
fun time_format(epoch_secs: int) -> string {
    // Euclidean division for correct negative handling
    let days = epoch_secs / 86400;
    let day_secs = epoch_secs - days * 86400;
    if day_secs < 0 {
        days = days - 1;
        day_secs = day_secs + 86400;
    }

    let hour = day_secs / 3600;
    let minute = (day_secs - hour * 3600) / 60;
    let second = day_secs - hour * 3600 - minute * 60;

    // civil_from_days: convert days since 1970-01-01 to y/m/d
    let z = days + 719468;
    let era = z / 146097;
    if z < 0 {
        era = (z - 146096) / 146097;
    }
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y_base = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + 3;
    if mp >= 10 {
        m = mp - 9;
    }
    let y = y_base;
    if m <= 2 {
        y = y_base + 1;
    }

    return _pad4(y) + "-" + _pad2(m) + "-" + _pad2(d) + " " + _pad2(hour) + ":" + _pad2(minute) + ":" + _pad2(second);
}

// ============================================================================
// String Operations
// ============================================================================

// Concatenate two strings by copying character data into a new string.
@inline
fun string_concat(a: string, b: string) -> string {
    let a_ptr = __heap_load(a, 0);
    let a_len = __heap_load(a, 1);
    let b_ptr = __heap_load(b, 0);
    let b_len = __heap_load(b, 1);
    let total = a_len + b_len;
    let data = __alloc_heap(total);
    let i = 0;
    while i < a_len {
        __heap_store(data, i, __heap_load(a_ptr, i));
        i = i + 1;
    }
    while i < total {
        __heap_store(data, i, __heap_load(b_ptr, i - a_len));
        i = i + 1;
    }
    return __alloc_string(data, total);
}

// Join all strings in an array into a single string.
// Pre-allocates the result buffer based on total length, then copies all parts.
@inline
fun string_join(parts: array<string>) -> string {
    let n = len(parts);
    let total = 0;
    let i = 0;
    while i < n {
        total = total + len(parts[i]);
        i = i + 1;
    }
    let data = __alloc_heap(total);
    let off = 0;
    i = 0;
    while i < n {
        let s = parts[i];
        let s_ptr = __heap_load(s, 0);
        let s_len = __heap_load(s, 1);
        let j = 0;
        while j < s_len {
            __heap_store(data, off, __heap_load(s_ptr, j));
            off = off + 1;
            j = j + 1;
        }
        i = i + 1;
    }
    return __alloc_string(data, total);
}

// ============================================================================
// High-level I/O Functions
// ============================================================================

// Print a string to stdout without a newline.
fun print_str(s: string) {
    let n = len(s);
    write(1, s, n);
}

// Print a string to stderr without a newline.
fun eprint_str(s: string) {
    let n = len(s);
    write(2, s, n);
}

// ============================================================================
// Testing / Assertion Functions
// ============================================================================

// Assert that a condition is true. If false, throws an error with the given message.
fun assert(condition: bool, msg: string) {
    if !condition {
        throw msg;
    }
}

// Assert that two values are equal. If not equal, throws an error with the given message.
// Uses to_string for comparison, so works with any type that can be converted to string.
fun assert_eq(actual: int, expected: int, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + to_string(expected) + ", actual: " + to_string(actual) + ")";
    }
}

// Assert that two strings are equal.
fun assert_eq_str(actual: string, expected: string, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + expected + ", actual: " + actual + ")";
    }
}

// Assert that two booleans are equal.
fun assert_eq_bool(actual: bool, expected: bool, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + to_string(expected) + ", actual: " + to_string(actual) + ")";
    }
}

// ============================================================================
// Math Functions
// ============================================================================

fun abs(x: int) -> int {
    if x < 0 {
        return -x;
    }
    return x;
}

fun max(a: int, b: int) -> int {
    if a > b {
        return a;
    }
    return b;
}

fun min(a: int, b: int) -> int {
    if a < b {
        return a;
    }
    return b;
}

// Internal: Convert float to int (truncation toward zero)
fun _float_to_int(x: float) -> int {
    return asm(x) -> i64 {
        __emit("I64TruncF64S");
    };
}

// Absolute value of a float
fun abs_f(x: float) -> float {
    if x < 0.0 {
        return 0.0 - x;
    }
    return x;
}

// Square root using Newton's method (Babylonian method)
fun sqrt_f(x: float) -> float {
    if x <= 0.0 {
        return 0.0;
    }
    let guess = x;
    // Better initial guess: halve repeatedly until reasonable
    if x > 1.0 {
        guess = x / 2.0;
    }
    let i = 0;
    while i < 20 {
        guess = (guess + x / guess) / 2.0;
        i = i + 1;
    }
    return guess;
}

// Floor: largest integer <= x, returned as float
fun floor_f(x: float) -> float {
    let t = _float_to_int(x);
    let tf = _int_to_float(t);
    // _float_to_int truncates toward zero, so for negative non-integers we need -1
    if tf > x {
        return tf - 1.0;
    }
    return tf;
}

// Float modulo (equivalent to fmod)
fun fmod_f(x: float, y: float) -> float {
    return x - floor_f(x / y) * y;
}

// Sine function using Taylor series with range reduction
fun sin_f(x: float) -> float {
    let pi = 3.14159265358979323846;
    let two_pi = 6.28318530717958647692;

    // Range reduction to [-pi, pi]
    let a = fmod_f(x, two_pi);
    if a > pi {
        a = a - two_pi;
    }
    if a < 0.0 - pi {
        a = a + two_pi;
    }

    // Taylor series: sin(a) = a - a^3/3! + a^5/5! - a^7/7! + ...
    let term = a;
    let sum = a;
    let i = 1;
    while i < 12 {
        let n = _int_to_float(2 * i) * (_int_to_float(2 * i) + 1.0);
        term = 0.0 - term * a * a / n;
        sum = sum + term;
        i = i + 1;
    }
    return sum;
}

// Cosine function: cos(x) = sin(x + pi/2)
fun cos_f(x: float) -> float {
    return sin_f(x + 1.5707963267948966);
}

// ============================================================================
// String Functions
// ============================================================================

fun str_len(s: string) -> int {
    return len(s);
}

fun str_contains(haystack: string, needle: string) -> bool {
    let haystack_len = len(haystack);
    let needle_len = len(needle);

    if needle_len == 0 {
        return true;
    }
    if needle_len > haystack_len {
        return false;
    }

    let i = 0;
    while i <= haystack_len - needle_len {
        let j = 0;
        let found = true;
        while j < needle_len {
            if haystack[i + j] != needle[j] {
                found = false;
                j = needle_len;
            } else {
                j = j + 1;
            }
        }
        if found {
            return true;
        }
        i = i + 1;
    }
    return false;
}

// Find the index of needle in haystack, returns -1 if not found
fun str_index_of(haystack: string, needle: string) -> int {
    let haystack_len = len(haystack);
    let needle_len = len(needle);

    if needle_len == 0 {
        return 0;
    }
    if needle_len > haystack_len {
        return -1;
    }

    let i = 0;
    while i <= haystack_len - needle_len {
        let j = 0;
        let found = true;
        while j < needle_len {
            if haystack[i + j] != needle[j] {
                found = false;
                j = needle_len;
            } else {
                j = j + 1;
            }
        }
        if found {
            return i;
        }
        i = i + 1;
    }
    return -1;
}

// ============================================================================
// Array Functions (fixed-length array using heap intrinsics)
// ============================================================================

// Array<T> - Fixed-length array implementation.
// Layout: [ptr, len]
struct Array<T> {
    ptr: int,
    len: int
}

impl<T> Array<T> {
    // Get a value at the specified index
    fun get(self, index: int) -> T {
        return __heap_load(self.ptr, index);
    }

    // Set a value at the specified index
    fun set(self, index: int, value: T) {
        __heap_store(self.ptr, index, value);
    }

    // Get the length of the array
    fun len(self) -> int {
        return self.len;
    }
}

// ============================================================================
// Vector Functions (low-level implementation using heap intrinsics)
// ============================================================================

// Vec<T> - Generic vector (dynamic array) implementation.
// Layout: [ptr, len, cap]
struct Vec<T> {
    ptr: int,
    len: int,
    cap: int
}

impl<T> Vec<T> {
    // Create a new empty vector.
    fun `new`() -> Vec<T> {
        return Vec<T> { ptr: 0, len: 0, cap: 0 };
    }

    // Create a vector with pre-set capacity.
    fun with_capacity(cap: int) -> Vec<T> {
        return Vec<T> { ptr: 0, len: 0, cap: cap };
    }

    // Create an uninitialized vector with specified length (for desugar).
    // The vector is allocated with the given capacity and length is set to capacity.
    // Elements are uninitialized and must be set before use.
    fun uninit(cap: int) -> Vec<T> {
        if cap == 0 {
            return Vec<T> { ptr: 0, len: 0, cap: 0 };
        }
        let data = __alloc_heap(cap);
        return Vec<T> { ptr: data, len: cap, cap: cap };
    }

    // Push a value to the end of the vector
    fun push(self, value: T) {
        if self.len >= self.cap {
            // Need to grow
            let new_cap = self.cap * 2;
            if new_cap < 8 {
                new_cap = 8;
            }
            let new_data = __alloc_heap(new_cap);

            // Copy old data if ptr is not null
            if self.ptr != 0 {
                let i = 0;
                while i < self.len {
                    let val = __heap_load(self.ptr, i);
                    __heap_store(new_data, i, val);
                    i = i + 1;
                }
            }

            // Update vector header
            self.ptr = new_data;
            self.cap = new_cap;
        }

        // Store the value at ptr[len]
        __heap_store(self.ptr, self.len, value);
        // Increment len
        self.len = self.len + 1;
    }

    // Pop a value from the end of the vector
    // Returns the popped value, throws if vector is empty.
    fun pop(self) -> T {
        if self.len == 0 {
            throw "cannot pop from empty vector";
        }

        self.len = self.len - 1;
        let value = __heap_load(self.ptr, self.len);

        return value;
    }

    // Get a value at the specified index
    @inline
    fun get(self, index: int) -> T {
        return __heap_load(self.ptr, index);
    }

    // Set a value at the specified index
    @inline
    fun set(self, index: int, value: T) {
        __heap_store(self.ptr, index, value);
    }

    // Get the length of the vector
    fun len(self) -> int {
        return self.len;
    }
}

// Associated functions for vec<T> (syntax sugar for Vec<T>)
impl vec {
    // Create a new empty vector.
    fun `new`() -> vec<any> {
        return Vec<any> { ptr: 0, len: 0, cap: 0 };
    }

    // Create a vector with pre-set capacity.
    fun with_capacity(cap: int) -> vec<any> {
        return Vec<any> { ptr: 0, len: 0, cap: cap };
    }
}

// ============================================================================
// Map Functions (HashMap implementation using chaining)
// ============================================================================

// HashMapEntry struct - represents a key-value pair in the map.
// Layout: [hm_key, hm_value, hm_next]
// hm_next: pointer to next entry in the chain (0 if end of chain)
struct HashMapEntry {
    hm_key: any,
    hm_value: any,
    hm_next: int
}

// Map<K, V> - Generic hash map implementation.
// Layout: [hm_buckets, hm_size, hm_capacity]
// hm_buckets: pointer to array of bucket heads
// hm_size: number of entries in the map
// hm_capacity: number of buckets
struct Map<K, V> {
    hm_buckets: int,
    hm_size: int,
    hm_capacity: int
}

// Hash function for integers - uses the value directly
fun _map_hash_int(key: int) -> int {
    if key < 0 {
        return -key;
    }
    return key;
}

// Hash function for strings - DJB2 algorithm
fun _map_hash_string(key: string) -> int {
    let hash = 5381;
    let n = len(key);
    let i = 0;
    while i < n {
        let c = key[i];
        // hash = hash * 33 + c
        hash = hash * 33 + c;
        i = i + 1;
    }
    if hash < 0 {
        return -hash;
    }
    return hash;
}

// Internal: Find entry by key in a bucket chain (int key)
fun _map_find_entry_int(m: Map<any, any>, key: int) -> int {
    let bucket_idx = _map_hash_int(key) % m.hm_capacity;
    let entry_ptr = __heap_load(m.hm_buckets, bucket_idx);

    while entry_ptr != 0 {
        let entry_key = __heap_load(entry_ptr, 0);
        if entry_key == key {
            return entry_ptr;
        }
        entry_ptr = __heap_load(entry_ptr, 2);
    }
    return 0;
}

// Internal: Find entry by key in a bucket chain (string key)
fun _map_find_entry_string(m: Map<any, any>, key: string) -> int {
    let bucket_idx = _map_hash_string(key) % m.hm_capacity;
    let entry_ptr = __heap_load(m.hm_buckets, bucket_idx);

    while entry_ptr != 0 {
        let entry_key = __heap_load(entry_ptr, 0);
        if entry_key == key {
            return entry_ptr;
        }
        entry_ptr = __heap_load(entry_ptr, 2);
    }
    return 0;
}

// Internal: Rehash the map when load factor exceeds 0.75 (int keys)
fun _map_rehash_int(m: Map<any, any>) {
    let old_capacity = m.hm_capacity;
    let old_buckets = m.hm_buckets;
    let new_capacity = old_capacity * 2;
    let new_buckets = __alloc_heap(new_capacity);

    // Initialize new buckets to 0
    let i = 0;
    while i < new_capacity {
        __heap_store(new_buckets, i, 0);
        i = i + 1;
    }

    // Rehash all entries
    i = 0;
    while i < old_capacity {
        let entry_ptr = __heap_load(old_buckets, i);
        while entry_ptr != 0 {
            let key = __heap_load(entry_ptr, 0);
            let next_ptr = __heap_load(entry_ptr, 2);

            // Compute new bucket index
            let new_bucket_idx = _map_hash_int(key) % new_capacity;

            // Insert at head of new bucket
            let old_head = __heap_load(new_buckets, new_bucket_idx);
            __heap_store(entry_ptr, 2, old_head);
            __heap_store(new_buckets, new_bucket_idx, entry_ptr);

            entry_ptr = next_ptr;
        }
        i = i + 1;
    }

    m.hm_buckets = new_buckets;
    m.hm_capacity = new_capacity;
}

// Internal: Rehash for string keys
fun _map_rehash_string(m: Map<any, any>) {
    let old_capacity = m.hm_capacity;
    let old_buckets = m.hm_buckets;
    let new_capacity = old_capacity * 2;
    let new_buckets = __alloc_heap(new_capacity);

    // Initialize new buckets to 0
    let i = 0;
    while i < new_capacity {
        __heap_store(new_buckets, i, 0);
        i = i + 1;
    }

    // Rehash all entries
    i = 0;
    while i < old_capacity {
        let entry_ptr = __heap_load(old_buckets, i);
        while entry_ptr != 0 {
            let key = __heap_load(entry_ptr, 0);
            let next_ptr = __heap_load(entry_ptr, 2);

            // Compute new bucket index
            let new_bucket_idx = _map_hash_string(key) % new_capacity;

            // Insert at head of new bucket
            let old_head = __heap_load(new_buckets, new_bucket_idx);
            __heap_store(entry_ptr, 2, old_head);
            __heap_store(new_buckets, new_bucket_idx, entry_ptr);

            entry_ptr = next_ptr;
        }
        i = i + 1;
    }

    m.hm_buckets = new_buckets;
    m.hm_capacity = new_capacity;
}

// Internal helper: push to vector without using method syntax
fun _vec_push_internal(v: Vec<any>, value) {
    if v.len >= v.cap {
        // Need to grow
        let new_cap = v.cap * 2;
        if new_cap < 8 {
            new_cap = 8;
        }
        let new_data = __alloc_heap(new_cap);

        // Copy old data if ptr is not null
        if v.ptr != 0 {
            let i = 0;
            while i < v.len {
                let val = __heap_load(v.ptr, i);
                __heap_store(new_data, i, val);
                i = i + 1;
            }
        }

        // Update vector header
        v.ptr = new_data;
        v.cap = new_cap;
    }

    // Store the value at ptr[len]
    __heap_store(v.ptr, v.len, value);
    // Increment len
    v.len = v.len + 1;
}

impl<K, V> Map<K, V> {
    // Create a new empty map
    fun `new`() -> Map<K, V> {
        let capacity = 16;
        let buckets = __alloc_heap(capacity);
        // Initialize all buckets to 0 (empty)
        let i = 0;
        while i < capacity {
            __heap_store(buckets, i, 0);
            i = i + 1;
        }
        return Map<K, V> { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
    }

    // Create an uninitialized empty map (for desugar).
    // Same as `new()` - elements will be added via put.
    fun uninit() -> Map<K, V> {
        let capacity = 16;
        let buckets = __alloc_heap(capacity);
        let i = 0;
        while i < capacity {
            __heap_store(buckets, i, 0);
            i = i + 1;
        }
        return Map<K, V> { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
    }

    // Put a key-value pair into the map (int key version)
    fun put_int(self, key: int, val: V) {
        // Check if key already exists
        let existing = _map_find_entry_int(self, key);
        if existing != 0 {
            // Update existing entry
            __heap_store(existing, 1, val);
            return;
        }

        // Check if we need to rehash (load factor > 0.75)
        let load = self.hm_size * 4;
        let threshold = self.hm_capacity * 3;
        if load >= threshold {
            _map_rehash_int(self);
        }

        // Create new entry
        let entry = __alloc_heap(3);
        __heap_store(entry, 0, key);
        __heap_store(entry, 1, val);

        // Insert at head of bucket
        let bucket_idx = _map_hash_int(key) % self.hm_capacity;
        let old_head = __heap_load(self.hm_buckets, bucket_idx);
        __heap_store(entry, 2, old_head);
        __heap_store(self.hm_buckets, bucket_idx, entry);

        self.hm_size = self.hm_size + 1;
    }

    // Put a key-value pair into the map (string key version)
    fun put_string(self, key: string, val) {
        // Check if key already exists
        let existing = _map_find_entry_string(self, key);
        if existing != 0 {
            // Update existing entry
            __heap_store(existing, 1, val);
            return;
        }

        // Check if we need to rehash (load factor > 0.75)
        let load = self.hm_size * 4;
        let threshold = self.hm_capacity * 3;
        if load >= threshold {
            _map_rehash_string(self);
        }

        // Create new entry
        let entry = __alloc_heap(3);
        __heap_store(entry, 0, key);
        __heap_store(entry, 1, val);

        // Insert at head of bucket
        let bucket_idx = _map_hash_string(key) % self.hm_capacity;
        let old_head = __heap_load(self.hm_buckets, bucket_idx);
        __heap_store(entry, 2, old_head);
        __heap_store(self.hm_buckets, bucket_idx, entry);

        self.hm_size = self.hm_size + 1;
    }

    // Get a value from the map by int key
    // Returns 0 if key not found
    fun get_int(self, key: int) {
        let entry_ptr = _map_find_entry_int(self, key);
        if entry_ptr == 0 {
            return 0;
        }
        return __heap_load(entry_ptr, 1);
    }

    // Get a value from the map by string key
    // Returns 0 if key not found
    fun get_string(self, key: string) {
        let entry_ptr = _map_find_entry_string(self, key);
        if entry_ptr == 0 {
            return 0;
        }
        return __heap_load(entry_ptr, 1);
    }

    // Check if the map contains a key (int version)
    fun contains_int(self, key: int) -> bool {
        return _map_find_entry_int(self, key) != 0;
    }

    // Check if the map contains a key (string version)
    fun contains_string(self, key: string) -> bool {
        return _map_find_entry_string(self, key) != 0;
    }

    // Remove an entry from the map by int key
    // Returns true if the key was found and removed, false otherwise
    fun remove_int(self, key: int) -> bool {
        let bucket_idx = _map_hash_int(key) % self.hm_capacity;
        let entry_ptr = __heap_load(self.hm_buckets, bucket_idx);
        let prev_ptr = 0;

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                // Found the entry, remove it
                let next_ptr = __heap_load(entry_ptr, 2);
                if prev_ptr == 0 {
                    // Entry is head of bucket
                    __heap_store(self.hm_buckets, bucket_idx, next_ptr);
                } else {
                    // Entry is in middle/end of chain
                    __heap_store(prev_ptr, 2, next_ptr);
                }
                self.hm_size = self.hm_size - 1;
                return true;
            }
            prev_ptr = entry_ptr;
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return false;
    }

    // Remove an entry from the map by string key
    // Returns true if the key was found and removed, false otherwise
    fun remove_string(self, key: string) -> bool {
        let bucket_idx = _map_hash_string(key) % self.hm_capacity;
        let entry_ptr = __heap_load(self.hm_buckets, bucket_idx);
        let prev_ptr = 0;

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                // Found the entry, remove it
                let next_ptr = __heap_load(entry_ptr, 2);
                if prev_ptr == 0 {
                    // Entry is head of bucket
                    __heap_store(self.hm_buckets, bucket_idx, next_ptr);
                } else {
                    // Entry is in middle/end of chain
                    __heap_store(prev_ptr, 2, next_ptr);
                }
                self.hm_size = self.hm_size - 1;
                return true;
            }
            prev_ptr = entry_ptr;
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return false;
    }

    // Get all keys from the map as a vector (works for any key type)
    fun keys(self) -> vec<any> {
        let result: Vec<any> = Vec<any> { ptr: 0, len: 0, cap: 0 };
        let i = 0;
        while i < self.hm_capacity {
            let entry_ptr = __heap_load(self.hm_buckets, i);
            while entry_ptr != 0 {
                let key = __heap_load(entry_ptr, 0);
                _vec_push_internal(result, key);
                entry_ptr = __heap_load(entry_ptr, 2);
            }
            i = i + 1;
        }
        return result;
    }

    // Get all values from the map as a vector
    fun values(self) -> vec<any> {
        let result: Vec<any> = Vec<any> { ptr: 0, len: 0, cap: 0 };
        let i = 0;
        while i < self.hm_capacity {
            let entry_ptr = __heap_load(self.hm_buckets, i);
            while entry_ptr != 0 {
                let val = __heap_load(entry_ptr, 1);
                _vec_push_internal(result, val);
                entry_ptr = __heap_load(entry_ptr, 2);
            }
            i = i + 1;
        }
        return result;
    }

    // Generic put method - dispatches based on key type
    fun put(self, key: any, val: any) {
        let key_type = type_of(key);
        if key_type == "int" {
            self.put_int(key, val);
        } else if key_type == "string" {
            self.put_string(key, val);
        } else {
            throw "map.put: unsupported key type";
        }
    }

    // Alias for put - used by index assignment desugar (map[key] = value)
    fun set(self, key: any, val: any) {
        self.put(key, val);
    }

    // Generic get method - dispatches based on key type
    fun get(self, key: any) -> any {
        let key_type = type_of(key);
        if key_type == "int" {
            return self.get_int(key);
        } else if key_type == "string" {
            return self.get_string(key);
        } else {
            throw "map.get: unsupported key type";
        }
    }

    // Generic contains method - dispatches based on key type
    fun contains(self, key: any) -> bool {
        let key_type = type_of(key);
        if key_type == "int" {
            return self.contains_int(key);
        }
        if key_type == "string" {
            return self.contains_string(key);
        }
        // Unsupported key type - throw error and return false to satisfy type checker
        throw "map.contains: unsupported key type";
        return false;
    }

    // Generic remove method - dispatches based on key type
    fun remove(self, key: any) -> bool {
        let key_type = type_of(key);
        if key_type == "int" {
            return self.remove_int(key);
        }
        if key_type == "string" {
            return self.remove_string(key);
        }
        // Unsupported key type - throw error and return false to satisfy type checker
        throw "map.remove: unsupported key type";
        return false;
    }

    // Get the size of the map
    fun len(self) -> int {
        return self.hm_size;
    }
}

// Associated functions for map<K, V> (syntax sugar for Map<K, V>)
impl map {
    // Create a new empty map with default capacity (16 buckets)
    fun `new`() -> map<any, any> {
        let capacity = 16;
        let buckets = __alloc_heap(capacity);
        // Initialize all buckets to 0 (nil)
        let i = 0;
        while i < capacity {
            __heap_store(buckets, i, 0);
            i = i + 1;
        }
        return Map<any, any> { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
    }
}

// ============================================================================
// Random Number Generation (LCG - Linear Congruential Generator)
// ============================================================================

// Internal: Convert int to float using inline assembly
fun _int_to_float(n: int) -> float {
    return asm(n) -> f64 {
        __emit("F64ConvertI64S");
    };
}

// Rand - Pseudo-random number generator using LCG algorithm.
// LCG parameters: a = 1103515245, c = 12345, m = 2147483648 (2^31)
//
// Usage:
//   let rng = Rand::new(42);
//   print(rng.int(1, 100));
//   print(rng.float());
struct Rand {
    _seed: int
}

impl Rand {
    // Create a new random number generator with the given seed.
    fun `new`(seed: int) -> Rand {
        return Rand { _seed: seed };
    }

    // Set the seed for the random number generator.
    fun set_seed(self, n: int) {
        self._seed = n;
    }

    // Generate the next raw random integer in [0, 2^31).
    fun next(self) -> int {
        self._seed = (self._seed * 1103515245 + 12345) % 2147483648;
        if self._seed < 0 {
            self._seed = -self._seed;
        }
        return self._seed;
    }

    // Generate a random integer in [min_val, max_val].
    // Throws an error if min_val > max_val.
    // Uses scaling (upper bits) instead of modulo to avoid LCG lower-bit bias.
    fun int(self, min_val: int, max_val: int) -> int {
        if min_val > max_val {
            throw "rand_int: min must be <= max";
        }
        let r = self.next();
        let range = max_val - min_val + 1;
        return min_val + r * range / 2147483648;
    }

    // Generate a random float in [0.0, 1.0).
    fun float(self) -> float {
        let r = self.next();
        return _int_to_float(r) / 2147483648.0;
    }
}

// ============================================================================
// Parsing Functions
// ============================================================================

// Check if a byte is a whitespace character (space, tab, newline, carriage return)
fun _is_whitespace(c: int) -> bool {
    return c == 32 || c == 9 || c == 10 || c == 13;
}

// Check if a byte is a digit ('0'-'9')
fun _is_digit(c: int) -> bool {
    return c >= 48 && c <= 57;
}

// Parse a string to an integer.
// Handles leading/trailing whitespace and optional negative sign.
// Throws an error if the string cannot be parsed as an integer.
fun std_parse_int(s: string) -> int {
    let n = len(s);
    let i = 0;

    // Skip leading whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    if i >= n {
        throw "cannot parse empty string as int";
    }

    // Check for negative sign
    let negative = false;
    if s[i] == 45 {
        negative = true;
        i = i + 1;
    }

    if i >= n || !_is_digit(s[i]) {
        throw "cannot parse '" + s + "' as int";
    }

    // Parse digits
    let result = 0;
    while i < n && _is_digit(s[i]) {
        let digit = s[i] - 48;
        result = result * 10 + digit;
        i = i + 1;
    }

    // Skip trailing whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    // Check for trailing non-whitespace characters
    if i < n {
        throw "cannot parse '" + s + "' as int";
    }

    if negative {
        return -result;
    }
    return result;
}

// ============================================================================
// Sort Functions (Quicksort with median-of-three pivot)
// ============================================================================

// Internal: swap two elements in a vec<int>
fun _sort_int_swap(v: Vec<int>, i: int, j: int) {
    let tmp = v[i];
    v[i] = v[j];
    v[j] = tmp;
}

// Internal: quicksort implementation for vec<int>
fun _sort_int_impl(v: Vec<int>, low: int, high: int) {
    if low >= high {
        return;
    }

    // Median-of-three pivot selection (only for 3+ elements)
    if high - low >= 2 {
        let mid = low + (high - low) / 2;
        if v[low] > v[mid] {
            _sort_int_swap(v, low, mid);
        }
        if v[low] > v[high] {
            _sort_int_swap(v, low, high);
        }
        if v[mid] > v[high] {
            _sort_int_swap(v, mid, high);
        }
        // v[mid] is the median, swap to high for Lomuto partition
        _sort_int_swap(v, mid, high);
    }

    // Lomuto partition with pivot at v[high]
    let pivot = v[high];
    let i = low;
    let j = low;
    while j < high {
        if v[j] <= pivot {
            _sort_int_swap(v, i, j);
            i = i + 1;
        }
        j = j + 1;
    }
    _sort_int_swap(v, i, high);

    // Recurse on both sides
    if i > low {
        _sort_int_impl(v, low, i - 1);
    }
    _sort_int_impl(v, i + 1, high);
}

// Sort a vec<int> in-place in ascending order using quicksort.
fun sort_int(v: Vec<int>) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    _sort_int_impl(v, 0, n - 1);
}

// Internal: swap two elements in a vec<float>
fun _sort_float_swap(v: Vec<float>, i: int, j: int) {
    let tmp = v[i];
    v[i] = v[j];
    v[j] = tmp;
}

// Internal: quicksort implementation for vec<float>
fun _sort_float_impl(v: Vec<float>, low: int, high: int) {
    if low >= high {
        return;
    }

    // Median-of-three pivot selection (only for 3+ elements)
    if high - low >= 2 {
        let mid = low + (high - low) / 2;
        if v[low] > v[mid] {
            _sort_float_swap(v, low, mid);
        }
        if v[low] > v[high] {
            _sort_float_swap(v, low, high);
        }
        if v[mid] > v[high] {
            _sort_float_swap(v, mid, high);
        }
        _sort_float_swap(v, mid, high);
    }

    // Lomuto partition with pivot at v[high]
    let pivot = v[high];
    let i = low;
    let j = low;
    while j < high {
        if v[j] <= pivot {
            _sort_float_swap(v, i, j);
            i = i + 1;
        }
        j = j + 1;
    }
    _sort_float_swap(v, i, high);

    // Recurse on both sides
    if i > low {
        _sort_float_impl(v, low, i - 1);
    }
    _sort_float_impl(v, i + 1, high);
}

// Sort a vec<float> in-place in ascending order using quicksort.
fun sort_float(v: Vec<float>) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    _sort_float_impl(v, 0, n - 1);
}
