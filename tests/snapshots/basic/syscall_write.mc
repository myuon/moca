// Test syscall_write to stdout
let result = syscall_write(1, "hello", 5);
print(result);

// Test partial write
let result2 = syscall_write(1, "world!", 3);
print(result2);

// Test with newline
syscall_write(1, "\n", 1);

// Test invalid fd (should return -1)
let bad_result = syscall_write(99, "test", 4);
print(bad_result);

// Test count larger than string length (should truncate)
let truncated = syscall_write(1, "hi", 100);
print(truncated);

syscall_write(1, "\n", 1);
