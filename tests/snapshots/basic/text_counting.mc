// Test: lorem ipsum character counting
// Counts each character occurrence and classifies characters by type.

// Classifies a character using only integer comparisons
// Returns: 0=space, 1=lowercase, 2=uppercase, 3=other
fun classify(ch: int) -> int {
    if ch == 32 { return 0; }
    if ch >= 97 {
        if ch <= 122 { return 1; }
    }
    if ch >= 65 {
        if ch <= 90 { return 2; }
    }
    return 3;
}

fun count_chars() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

    // Initialize counts array for ASCII 0-127
    var counts: vec<any> = vec::`new`();
    var k = 0;
    while k < 128 {
        counts.push(0);
        k = k + 1;
    }

    var spaces = 0;
    var lowercase = 0;
    var uppercase = 0;
    var other = 0;

    var i = 0;
    while i < len(text) {
        let ch = text[i];
        counts[ch] = counts[ch] + 1;
        let cls = classify(ch);
        if cls == 0 { spaces = spaces + 1; }
        if cls == 1 { lowercase = lowercase + 1; }
        if cls == 2 { uppercase = uppercase + 1; }
        if cls == 3 { other = other + 1; }
        i = i + 1;
    }

    // Output per-character counts in format "ascii_code: count"
    var c = 0;
    while c < 128 {
        if counts[c] > 0 {
            print(to_string(c) + ": " + to_string(counts[c]));
        }
        c = c + 1;
    }
    print(spaces);
    print(lowercase);
    print(uppercase);
    print(other);
}

count_chars();
