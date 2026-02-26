// Test file I/O operations using std/prelude functions

// Test 1: Create file, write, close, read back
let path = "/tmp/moca_file_io_test.txt";
let content = "Hello, Moca!";

// Open file for writing (O_WRONLY + O_CREAT + O_TRUNC = 1 + 64 + 512 = 577)
let fd = open(path, 577);
if fd < 0 {
    print("ERROR: Failed to open file for writing");
} else {
    // Write content
    let written = write_str(fd, content, len(content));
    print(written);

    // Close file
    let close_result = close(fd);
    print(close_result);
}

// Open file for reading (O_RDONLY = 0)
let fd2 = open(path, 0);
if fd2 < 0 {
    print("ERROR: Failed to open file for reading");
} else {
    // Read content back
    let read_content = read(fd2, 100);
    print(read_content);

    // Close file
    let close_result2 = close(fd2);
    print(close_result2);
}

// Test 2: Verify constants are accessible
print(O_RDONLY());
print(O_WRONLY());
print(O_CREAT());
print(O_TRUNC());
print(AF_INET());
print(SOCK_STREAM());
