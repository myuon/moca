// Test try-catch-throw statements
fun might_fail(x: int) -> int {
    if x < 0 {
        throw "negative value not allowed";
    }
    return x * 2;
}

// Test successful case
try {
    let result = might_fail(5);
    print($"{result}");
} catch e {
    print("caught error");
}

// Test error case
try {
    let result = might_fail(-1);
    print($"{result}");
} catch e {
    print("caught: " + e);
}

// Nested try-catch
try {
    try {
        throw "inner error";
    } catch e1 {
        print("inner caught: " + e1);
        throw "rethrown";
    }
} catch e2 {
    print("outer caught: " + e2);
}
