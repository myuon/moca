// Return at end of function (no warning)
fun foo() -> int {
    let x = 42;
    return x;
}

// Throw at end (no warning)
fun bar() {
    print("about to throw");
    throw "error";
}

// Break at end of loop body (no warning)
fun baz() {
    let _i = 0;
    while true {
        print("loop");
        break;
    }
}

// Continue at end of loop body (no warning)
fun qux() {
    while true {
        print("loop");
        continue;
    }
}

// Return in if/else but code after if block (no warning)
fun with_if() -> int {
    let x = 10;
    if x > 5 {
        return 1;
    }
    return 0;
}

// Empty function (no warning)
fun empty() {
}
