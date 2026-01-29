// Any type interoperability with other types

// int -> any
let a: int = 100;
let b: any = a;
print(b);

// any -> int
let c: any = 200;
let d: int = c;
print(d);

// any with arithmetic (any ~ int -> int)
let x: any = 10;
let y = x + 5;
print(y);

// any with string concatenation
let s: any = "hello";
let t = s + " world";
print(t);

// any with comparison
let p: any = 42;
let q = p == 42;
print(q);

// any ~ any
let m: any = 1;
let n: any = m;
print(n);
