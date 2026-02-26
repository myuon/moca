// Test write hostcall to stdout
let result = write_str(1,"hello", 5);
print(result);

// Test partial write
let result2 = write_str(1,"world!", 3);
print(result2);

// Test with newline
write_str(1,"\n", 1);

// Test invalid fd (should return -1)
let bad_result = write_str(99, "test", 4);
print(bad_result);

// Test count larger than string length (should truncate)
let truncated = write_str(1,"hi", 100);
print(truncated);

write_str(1,"\n", 1);

// Test stderr with write
write_str(2,"stderr_test", 11);
write_str(2,"\n", 1);

// Test eprint_str from stdlib
eprint_str("eprint_works");
eprint_str("\n");
