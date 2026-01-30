// Test I/O error handling

// Test 1: Write to invalid fd (should return EBADF = -1)
let result1 = write(99, "test", 4);
print(result1);

// Test 2: Read from invalid fd (should return EBADF = -1)
let result2 = read(99, 100);
print(result2);

// Test 3: Close invalid fd (should return EBADF = -1)
let result3 = close(99);
print(result3);

// Test 4: Close reserved fds (stdin=0, stdout=1, stderr=2) - should return EBADF
let result4 = close(0);
print(result4);
let result5 = close(1);
print(result5);
let result6 = close(2);
print(result6);

// Test 5: Open non-existent file for reading (should return ENOENT = -2)
let result7 = open("/nonexistent/path/file.txt", 0);
print(result7);

// Test 6: Socket with invalid domain (should return EAFNOSUPPORT = -6)
let result8 = socket(99, 1);
print(result8);

// Test 7: Socket with invalid type (should return ESOCKTNOSUPPORT = -7)
let result9 = socket(2, 99);
print(result9);

// Test 8: Connect with invalid fd (should return EBADF = -1)
let result10 = connect(99, "localhost", 80);
print(result10);

// Test 9: Verify error constants
print(EBADF());
print(ENOENT());
print(EACCES());
print(ECONNREFUSED());
print(ETIMEDOUT());
print(EAFNOSUPPORT());
print(ESOCKTNOSUPPORT());
