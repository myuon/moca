// Test generic method calls with explicit type arguments

struct Converter {
    scale: int
}

impl Converter {
    fun convert<U>(self, f: (int) -> U) -> U {
        return f(self.scale);
    }
}

let double = fun(n: int) -> int { return n * 2; };
let is_zero = fun(n: int) -> bool { return n == 0; };

let c = Converter { scale: 42 };
let d = c.convert<int>(double);
print(d);

let b = c.convert<bool>(is_zero);
print(b);
