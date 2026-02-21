// Test syscall 10 (time), 11 (time_nanos), 12 (time_format)

let secs = time();
let nanos = time_nanos();
let formatted = time_format(secs);
let epoch0 = time_format(0);

print(secs);
print(nanos);
print(formatted);
print(epoch0);
