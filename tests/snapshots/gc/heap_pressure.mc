// Test GC: Create many arrays that should be garbage collected
// With GC enabled, this should run successfully as old arrays are reclaimed
// With GC disabled and small heap limit, this should fail with heap limit exceeded

fun create_garbage() {
    var i = 0;
    while i < 1000 {
        // Create arrays that become garbage after each iteration
        let arr = [i, i + 1, i + 2, i + 3, i + 4];
        let arr2 = [arr];
        i = i + 1;
    }
}

// Run multiple rounds to generate more garbage
create_garbage();
create_garbage();
create_garbage();

print("done");
