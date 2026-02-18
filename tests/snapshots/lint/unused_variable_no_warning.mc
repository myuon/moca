// All variables are used — no warnings expected
let x = 42;
print($"{x}");

// _ prefix — no warning
let _unused = 99;

// Used in expression
let a = 1;
let b = 2;
let c = a + b;
print($"{c}");

// for-in with used variable
for i in [1, 2, 3] {
    print($"{i}");
}

// catch with used variable
fun safe() {
    try {
        print("try");
    } catch e {
        print($"{e}");
    }
}
