// HTTP GET client using CLI arguments
// Usage: moca run examples/http_get.mc <host> <port> <path>
// Example: moca run examples/http_get.mc httpbin.org 80 /get

fun main() {
    if argc() < 4 {
        print("Usage: moca run examples/http_get.mc <host> <port> <path>");
        print("Example: moca run examples/http_get.mc httpbin.org 80 /get");
        return 0;
    }

    let host = argv(1);
    let port = parse_int(argv(2));
    let path = argv(3);

    print("Connecting to " + host + ":" + port.to_string() + path);

    // Create TCP socket
    let fd = socket(AF_INET(), SOCK_STREAM());
    if fd < 0 {
        print("Error: Failed to create socket");
        return 1;
    }

    // Connect to host
    let result = connect(fd, host, port);
    if result < 0 {
        print("Error: Failed to connect");
        close(fd);
        return 1;
    }

    // Build HTTP request
    let request = "GET " + path + " HTTP/1.1\r\n" +
                  "Host: " + host + "\r\n" +
                  "Connection: close\r\n" +
                  "\r\n";

    // Send request
    let sent = write(fd, request, len(request));
    if sent < 0 {
        print("Error: Failed to send request");
        close(fd);
        return 1;
    }

    print("--- Response ---");

    // Read response
    let response = read(fd, 4096);
    while len(response) > 0 {
        print_str(response);
        response = read(fd, 4096);
    }

    close(fd);
    return 0;
}

main();
