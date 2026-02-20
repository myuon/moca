let ch = channel();
let sender_id = ch[0];
let receiver_id = ch[1];

// Send some values
send(sender_id, 42);
send(sender_id, 100);

// Receive them
let a = recv(receiver_id);
let b = recv(receiver_id);
print_debug(a);
print_debug(b);
