// Unreachable code after return
fun foo() -> int {
    return 42;
    print("unreachable");
}

// Unreachable code after throw
fun bar() {
    throw "error";
    let _x = 1;
}

// Unreachable code after break in while
fun baz() {
    while true {
        break;
        print("unreachable");
    }
}

// Unreachable code after continue in while
fun qux() {
    while true {
        continue;
        print("unreachable");
    }
}

// Unreachable code in if block
fun with_if() {
    if true {
        return;
        print("unreachable in if");
    }
}

// Multiple unreachable statements (only first is reported)
fun multi() {
    return;
    let _a = 2;
    let _b = 3;
}
