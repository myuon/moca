// Test: Backtick-escaped identifier for reserved words
// Expected output:
// 42
// hello
// 100

// Define a function named `new` using backtick escape
fun `new`() -> int {
    return 42;
}

// Call the escaped function
print($"{`new`()}");

// Define a function named `let` using backtick escape
fun `let`(s: string) {
    print($"{s}");
}

// Call the escaped function
`let`("hello");

// Variable using escaped identifier
let `if` = 100;
print($"{`if`}");
