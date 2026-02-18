// const basic test
const N = 42;
print($"{N}");

const PI = 3.14;
print($"{PI}");

const NAME = "hello";
print($"{NAME}");

const FLAG = true;
print($"{FLAG}");

// const in function scope
fun test() {
    const LOCAL = 100;
    print($"{LOCAL}");
}
test();

// const in block scope
if true {
    const BLOCK = 999;
    print($"{BLOCK}");
}

// const used in expressions
const A = 10;
const B = 20;
print($"{A + B}");

// const shadowing
const X = 1;
print($"{X}");
let X = 2;
print($"{X}");
