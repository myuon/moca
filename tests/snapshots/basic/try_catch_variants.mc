// Test try-catch with different exception types
fun might_throw(should_throw: bool) {
    if should_throw {
        throw "error occurred";
    }
    print("no error");
}

// Test 1: No exception
try {
    might_throw(false);
    print("after no throw");
} catch e {
    print("caught: " + e);
}

// Test 2: Exception caught
try {
    might_throw(true);
    print("this should not print");
} catch e {
    print("caught: " + e);
}

// Test 3: Nested try-catch
try {
    try {
        throw "inner error";
    } catch e {
        print("inner catch: " + e);
        throw "rethrow";
    }
} catch e {
    print("outer catch: " + e);
}
