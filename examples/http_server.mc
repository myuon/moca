// Simple HTTP server example
// Usage: moca run examples/http_server.mc [port]
// Example: moca run examples/http_server.mc 8080
// Then access: curl http://localhost:8080/

fun main() {
    let port = 8080;
    if argc() >= 2 {
        port = parse_int(argv(1));
    }

    print("Starting HTTP server on port " + port.to_string());

    // Create TCP socket
    let fd = socket(AF_INET(), SOCK_STREAM());
    if fd < 0 {
        print("Error: Failed to create socket");
        return 1;
    }

    // Bind to address
    let bind_result = bind(fd, "0.0.0.0", port);
    if bind_result < 0 {
        print("Error: Failed to bind to port " + port.to_string());
        if bind_result == EADDRINUSE() {
            print("  Port is already in use");
        }
        close(fd);
        return 1;
    }

    // Start listening
    let listen_result = listen(fd, 10);
    if listen_result < 0 {
        print("Error: Failed to listen");
        close(fd);
        return 1;
    }

    print("Server listening on http://0.0.0.0:" + port.to_string());
    print("Press Ctrl+C to stop");

    // Accept and handle connections in a loop
    let running = true;
    while running {
        print("Waiting for connection...");

        // Accept a client connection
        let client_fd = accept(fd);
        if client_fd < 0 {
            print("Error: Failed to accept connection");
            running = false;
        } else {
            print("Client connected (fd=" + client_fd.to_string() + ")");

            // Read the HTTP request
            let request = read(client_fd, 4096);
            print("Received request:");
            print(request);

            // Build HTTP response
            let body = "Hello, World!\n";
            let response = "HTTP/1.1 200 OK\r\n" +
                          "Content-Type: text/plain\r\n" +
                          "Content-Length: " + len(body).to_string() + "\r\n" +
                          "Connection: close\r\n" +
                          "\r\n" +
                          body;

            // Send response
            write(client_fd, response, len(response));
            print("Response sent");

            // Close client connection
            close(client_fd);
            print("Client disconnected");
            print("");
        }
    }

    close(fd);
    return 0;
}

main();
