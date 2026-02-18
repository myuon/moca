// Test short-circuit evaluation of && and ||

// Helper functions that print when called
fun returns_true() -> bool {
    print("returns_true called");
    return true;
}

fun returns_false() -> bool {
    print("returns_false called");
    return false;
}

// Test && short-circuit: if left is false, right should NOT be evaluated
print("== && with false on left ==");
let r1 = returns_false() && returns_true();
print($"{r1}");

// Test && without short-circuit: if left is true, right should be evaluated
print("== && with true on left ==");
let r2 = returns_true() && returns_false();
print($"{r2}");

// Test || short-circuit: if left is true, right should NOT be evaluated
print("== || with true on left ==");
let r3 = returns_true() || returns_false();
print($"{r3}");

// Test || without short-circuit: if left is false, right should be evaluated
print("== || with false on left ==");
let r4 = returns_false() || returns_true();
print($"{r4}");

// Test chained &&
print("== chained && ==");
let r5 = returns_true() && returns_true() && returns_false();
print($"{r5}");

// Test chained ||
print("== chained || ==");
let r6 = returns_false() || returns_false() || returns_true();
print($"{r6}");
