// Test that throw in a function skips subsequent code including return

fun will_throw() -> int {
    print("in function - before throw");
    throw "function error";
    print("in function - after throw - should not print");
    return 42;
}

print("calling function");
let result = will_throw();
print("after function call - should not print");
print($"{result}");
