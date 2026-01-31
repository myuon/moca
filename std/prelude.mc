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

    var i = 0;
    while i <= haystack_len - needle_len {
        var j = 0;
        var found = true;
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

    var i = 0;
    while i <= haystack_len - needle_len {
        var j = 0;
        var found = true;
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
// Vector Functions (low-level implementation using heap intrinsics)
// ============================================================================

// VectorAny struct - compatible with vector internal layout.
// This allows treating vectors as structs for more natural field access.
// Layout: [ptr, len, cap]
struct VectorAny {
    ptr: int,
    len: int,
    cap: int
}

// Internal implementation of vec_new. Creates an empty vector.
// Uses VectorAny struct literal for cleaner code.
fun vec_new_any() {
    return VectorAny { ptr: 0, len: 0, cap: 0 };
}

// Internal implementation of vec_with_capacity. Creates a vector with pre-set capacity.
// Uses VectorAny struct literal for cleaner code.
fun vec_with_capacity_any(cap) {
    return VectorAny { ptr: 0, len: 0, cap: cap };
}

// Internal implementation of vec_push. The vec_push builtin calls this function.
// Uses VectorAny-compatible field access (v.ptr, v.len, v.cap).
fun vec_push_any(v, value) {
    var data_ptr = v.ptr;
    var current_len = v.len;
    var current_cap = v.cap;

    if current_len >= current_cap {
        // Need to grow
        var new_cap = current_cap * 2;
        if new_cap < 8 {
            new_cap = 8;
        }
        let new_data = __alloc_heap(new_cap);

        // Copy old data if data_ptr is not null
        if data_ptr != nil {
            var i = 0;
            while i < current_len {
                let val = __heap_load(data_ptr, i);
                __heap_store(new_data, i, val);
                i = i + 1;
            }
        }

        // Update vector header
        v.ptr = new_data;
        v.cap = new_cap;
        data_ptr = new_data;
    }

    // Store the value at data_ptr[current_len]
    __heap_store(data_ptr, current_len, value);
    // Increment len
    v.len = current_len + 1;
}

// Internal implementation of vec_pop. The vec_pop builtin calls this function.
// Uses VectorAny-compatible field access.
// Returns the popped value, throws if vector is empty.
fun vec_pop_any(v) {
    let current_len = v.len;

    if current_len == 0 {
        throw "cannot pop from empty vector";
    }

    let new_len = current_len - 1;
    let data_ptr = v.ptr;
    let value = __heap_load(data_ptr, new_len);

    // Update len
    v.len = new_len;

    return value;
}

// Internal implementation of vec_get. The vec_get builtin calls this function.
// Uses VectorAny-compatible field access.
fun vec_get_any(v, index) {
    return __heap_load(v.ptr, index);
}

// Internal implementation of vec_set. The vec_set builtin calls this function.
// Uses VectorAny-compatible field access.
fun vec_set_any(v, index, value) {
    __heap_store(v.ptr, index, value);
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

// HashMapAny struct - represents the hash map.
// Layout: [hm_buckets, hm_size, hm_capacity]
// hm_buckets: pointer to array of bucket heads
// hm_size: number of entries in the map
// hm_capacity: number of buckets
struct HashMapAny {
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
    var hash = 5381;
    let n = len(key);
    var i = 0;
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

// Create a new empty map with default capacity (16 buckets)
fun map_new_any() {
    let capacity = 16;
    let buckets = __alloc_heap(capacity);
    // Initialize all buckets to 0 (nil)
    var i = 0;
    while i < capacity {
        __heap_store(buckets, i, 0);
        i = i + 1;
    }
    return HashMapAny { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
}

// Internal: Find entry by key in a bucket chain (int key)
// Returns the entry pointer if found, 0 if not found
fun _map_find_entry_int(m, key: int) -> int {
    let capacity = m.hm_capacity;
    let bucket_idx = _map_hash_int(key) % capacity;
    var entry_ptr = __heap_load(m.hm_buckets, bucket_idx);

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
fun _map_find_entry_string(m, key: string) -> int {
    let capacity = m.hm_capacity;
    let bucket_idx = _map_hash_string(key) % capacity;
    var entry_ptr = __heap_load(m.hm_buckets, bucket_idx);

    while entry_ptr != 0 {
        let entry_key = __heap_load(entry_ptr, 0);
        if entry_key == key {
            return entry_ptr;
        }
        entry_ptr = __heap_load(entry_ptr, 2);
    }
    return 0;
}

// Internal: Rehash the map when load factor exceeds 0.75
fun _map_rehash_int(m) {
    let old_capacity = m.hm_capacity;
    let old_buckets = m.hm_buckets;
    let new_capacity = old_capacity * 2;
    let new_buckets = __alloc_heap(new_capacity);

    // Initialize new buckets to 0
    var i = 0;
    while i < new_capacity {
        __heap_store(new_buckets, i, 0);
        i = i + 1;
    }

    // Rehash all entries
    i = 0;
    while i < old_capacity {
        var entry_ptr = __heap_load(old_buckets, i);
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
fun _map_rehash_string(m) {
    let old_capacity = m.hm_capacity;
    let old_buckets = m.hm_buckets;
    let new_capacity = old_capacity * 2;
    let new_buckets = __alloc_heap(new_capacity);

    // Initialize new buckets to 0
    var i = 0;
    while i < new_capacity {
        __heap_store(new_buckets, i, 0);
        i = i + 1;
    }

    // Rehash all entries
    i = 0;
    while i < old_capacity {
        var entry_ptr = __heap_load(old_buckets, i);
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

// Put a key-value pair into the map (int key version)
fun map_put_int(m, key: int, val) {
    // Check if key already exists
    let existing = _map_find_entry_int(m, key);
    if existing != 0 {
        // Update existing entry
        __heap_store(existing, 1, val);
        return;
    }

    // Check if we need to rehash (load factor > 0.75)
    let load = m.hm_size * 4;
    let threshold = m.hm_capacity * 3;
    if load >= threshold {
        _map_rehash_int(m);
    }

    // Create new entry
    let entry = __alloc_heap(3);
    __heap_store(entry, 0, key);
    __heap_store(entry, 1, val);

    // Insert at head of bucket
    let bucket_idx = _map_hash_int(key) % m.hm_capacity;
    let old_head = __heap_load(m.hm_buckets, bucket_idx);
    __heap_store(entry, 2, old_head);
    __heap_store(m.hm_buckets, bucket_idx, entry);

    m.hm_size = m.hm_size + 1;
}

// Put a key-value pair into the map (string key version)
fun map_put_string(m, key: string, val) {
    // Check if key already exists
    let existing = _map_find_entry_string(m, key);
    if existing != 0 {
        // Update existing entry
        __heap_store(existing, 1, val);
        return;
    }

    // Check if we need to rehash (load factor > 0.75)
    let load = m.hm_size * 4;
    let threshold = m.hm_capacity * 3;
    if load >= threshold {
        _map_rehash_string(m);
    }

    // Create new entry
    let entry = __alloc_heap(3);
    __heap_store(entry, 0, key);
    __heap_store(entry, 1, val);

    // Insert at head of bucket
    let bucket_idx = _map_hash_string(key) % m.hm_capacity;
    let old_head = __heap_load(m.hm_buckets, bucket_idx);
    __heap_store(entry, 2, old_head);
    __heap_store(m.hm_buckets, bucket_idx, entry);

    m.hm_size = m.hm_size + 1;
}

// Get a value from the map by int key
// Returns 0 if key not found
fun map_get_int(m, key: int) {
    let entry_ptr = _map_find_entry_int(m, key);
    if entry_ptr == 0 {
        return 0;
    }
    return __heap_load(entry_ptr, 1);
}

// Get a value from the map by string key
// Returns 0 if key not found
fun map_get_string(m, key: string) {
    let entry_ptr = _map_find_entry_string(m, key);
    if entry_ptr == 0 {
        return 0;
    }
    return __heap_load(entry_ptr, 1);
}

// Check if the map contains a key (int version)
fun map_contains_int(m, key: int) -> bool {
    return _map_find_entry_int(m, key) != 0;
}

// Check if the map contains a key (string version)
fun map_contains_string(m, key: string) -> bool {
    return _map_find_entry_string(m, key) != 0;
}

// Remove an entry from the map by int key
// Returns true if the key was found and removed, false otherwise
fun map_remove_int(m, key: int) -> bool {
    let capacity = m.hm_capacity;
    let bucket_idx = _map_hash_int(key) % capacity;
    var entry_ptr = __heap_load(m.hm_buckets, bucket_idx);
    var prev_ptr = 0;

    while entry_ptr != 0 {
        let entry_key = __heap_load(entry_ptr, 0);
        if entry_key == key {
            // Found the entry, remove it
            let next_ptr = __heap_load(entry_ptr, 2);
            if prev_ptr == 0 {
                // Entry is head of bucket
                __heap_store(m.hm_buckets, bucket_idx, next_ptr);
            } else {
                // Entry is in middle/end of chain
                __heap_store(prev_ptr, 2, next_ptr);
            }
            m.hm_size = m.hm_size - 1;
            return true;
        }
        prev_ptr = entry_ptr;
        entry_ptr = __heap_load(entry_ptr, 2);
    }
    return false;
}

// Remove an entry from the map by string key
// Returns true if the key was found and removed, false otherwise
fun map_remove_string(m, key: string) -> bool {
    let capacity = m.hm_capacity;
    let bucket_idx = _map_hash_string(key) % capacity;
    var entry_ptr = __heap_load(m.hm_buckets, bucket_idx);
    var prev_ptr = 0;

    while entry_ptr != 0 {
        let entry_key = __heap_load(entry_ptr, 0);
        if entry_key == key {
            // Found the entry, remove it
            let next_ptr = __heap_load(entry_ptr, 2);
            if prev_ptr == 0 {
                // Entry is head of bucket
                __heap_store(m.hm_buckets, bucket_idx, next_ptr);
            } else {
                // Entry is in middle/end of chain
                __heap_store(prev_ptr, 2, next_ptr);
            }
            m.hm_size = m.hm_size - 1;
            return true;
        }
        prev_ptr = entry_ptr;
        entry_ptr = __heap_load(entry_ptr, 2);
    }
    return false;
}

// Get the number of entries in the map
fun map_len(m) -> int {
    return m.hm_size;
}

// Get all keys from the map as a vector (works for any key type)
fun map_keys(m) {
    let result = vec_new_any();
    let capacity = m.hm_capacity;
    var i = 0;
    while i < capacity {
        var entry_ptr = __heap_load(m.hm_buckets, i);
        while entry_ptr != 0 {
            let key = __heap_load(entry_ptr, 0);
            vec_push_any(result, key);
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        i = i + 1;
    }
    return result;
}

// Get all values from the map as a vector
fun map_values(m) {
    let result = vec_new_any();
    let capacity = m.hm_capacity;
    var i = 0;
    while i < capacity {
        var entry_ptr = __heap_load(m.hm_buckets, i);
        while entry_ptr != 0 {
            let val = __heap_load(entry_ptr, 1);
            vec_push_any(result, val);
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        i = i + 1;
    }
    return result;
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
    var i = 0;

    // Skip leading whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    if i >= n {
        throw "cannot parse empty string as int";
    }

    // Check for negative sign
    var negative = false;
    if s[i] == 45 {
        negative = true;
        i = i + 1;
    }

    if i >= n || !_is_digit(s[i]) {
        throw "cannot parse '" + s + "' as int";
    }

    // Parse digits
    var result = 0;
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
