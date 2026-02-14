// Basic unused variable
let x = 42;

// Used variable (no warning)
let y = 10;
print(y);

// _ prefix suppresses warning
let _z = 99;

// Unused in function
fun foo() {
    let a = 1;
    let b = 2;
    print(b);
}

// Unused for-in loop variable
for i in [1, 2, 3] {
    print("hello");
}

// Used for-in loop variable (no warning)
for j in [1, 2, 3] {
    print(j);
}

// Unused catch variable
fun bar() {
    try {
        print("try");
    } catch e {
        print("caught");
    }
}

// Used catch variable (no warning)
fun baz() {
    try {
        print("try");
    } catch e {
        print(e);
    }
}

// Nested scope: unused variable inside if
fun nested() {
    let used = true;
    if used {
        let inner = 42;
    }
}

// Multiple unused variables
let p = 1;
let q = 2;
let r = 3;
print(r);

// Assignment-only is NOT a usage
let w = 0;
w = 10;
