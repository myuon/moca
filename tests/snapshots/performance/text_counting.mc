// Benchmark: lorem ipsum character counting
// Counts letter frequency (case-insensitive, skipping spaces/punctuation)
// and prints top 10 most frequent letters.

// JIT-compilable: converts character to lowercase letter index (0-25)
// Returns -1 if not a letter
@inline
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
    let labels: Vec<string> = new Vec<string> {
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
        "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"
    };

    // Initialize letter counts (26 letters)
    let counts: Vec<int> = new Vec<int> {
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    };

    // Count letters across 40000 iterations (case-insensitive, skip non-letters)
    let iter = 0;
    while iter < 40000 {
        let i = 0;
        while i < len(text) {
            let ch = text[i];
            let idx = to_letter_index(ch);
            if idx >= 0 {
                counts[idx] = counts[idx] + 1;
            }
            i = i + 1;
        }
        iter = iter + 1;
    }

    // Find and print top 10 by frequency
    let rank = 0;
    while rank < 10 {
        let max_idx = 0;
        let max_val = counts[0];
        let j = 1;
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
