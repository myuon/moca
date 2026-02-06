// Test: lorem ipsum character counting
// Counts letter frequency (case-insensitive, skipping spaces/punctuation)
// and prints top 10 most frequent letters.

// Converts character to lowercase letter index (0-25)
// Returns -1 if not a letter
fun to_letter_index(ch: int) -> int {
    if ch >= 65 {
        if ch <= 90 { return ch - 65; }
    }
    if ch >= 97 {
        if ch <= 122 { return ch - 97; }
    }
    return 0 - 1;
}

fun count_chars() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

    // Labels for output
    var labels: vec<any> = vec::`new`();
    labels.push("A"); labels.push("B"); labels.push("C"); labels.push("D");
    labels.push("E"); labels.push("F"); labels.push("G"); labels.push("H");
    labels.push("I"); labels.push("J"); labels.push("K"); labels.push("L");
    labels.push("M"); labels.push("N"); labels.push("O"); labels.push("P");
    labels.push("Q"); labels.push("R"); labels.push("S"); labels.push("T");
    labels.push("U"); labels.push("V"); labels.push("W"); labels.push("X");
    labels.push("Y"); labels.push("Z");

    // Initialize letter counts (26 letters)
    var counts: vec<any> = vec::`new`();
    var k = 0;
    while k < 26 {
        counts.push(0);
        k = k + 1;
    }

    // Count letters (case-insensitive, skip non-letters)
    var i = 0;
    while i < len(text) {
        let ch = text[i];
        let idx = to_letter_index(ch);
        if idx >= 0 {
            counts[idx] = counts[idx] + 1;
        }
        i = i + 1;
    }

    // Find and print top 10 by frequency
    var rank = 0;
    while rank < 10 {
        var max_idx = 0;
        var max_val = counts[0];
        var j = 1;
        while j < 26 {
            if counts[j] > max_val {
                max_val = counts[j];
                max_idx = j;
            }
            j = j + 1;
        }
        print(labels[max_idx] + ": " + to_string(max_val));
        counts[max_idx] = 0 - 1;
        rank = rank + 1;
    }
}

count_chars();
