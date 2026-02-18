// Test variable shadowing in different scopes
let x = 10;
print($"{x}");

// Shadowing within if block
if true {
    let x = 20;
    print($"{x}");
    if true {
        let x = 30;
        print($"{x}");
    }
    print($"{x}");
}
print($"{x}");

// Shadowing within while loop
let i = 0;
while i < 1 {
    let x = 50;
    print($"{x}");
    i = i + 1;
}
print($"{x}");

// Function parameter shadows outer
fun test(x: int) -> int {
    return x + 1;
}
print($"{test(100)}");
print($"{x}");
