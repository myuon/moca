// Test match dyn with interface arms (vtable-based matching)

interface Describable {
    fun describe(self) -> string;
}

struct Point {
    x: int,
    y: int
}

impl Describable for Point {
    fun describe(self) -> string {
        return "Point(" + self.x.to_string() + ", " + self.y.to_string() + ")";
    }
}

struct Color {
    r: int,
    g: int,
    b: int
}

impl Describable for Color {
    fun describe(self) -> string {
        return "rgb(" + self.r.to_string() + "," + self.g.to_string() + "," + self.b.to_string() + ")";
    }
}

// match dyn with interface arm: matches any type implementing Describable
fun show(d: dyn) -> string {
    match dyn d {
        v: int => {
            return "int:" + v.to_string();
        }
        v: Describable => {
            return "describable";
        }
        _ => {
            return "other";
        }
    }
}

let p = Point { x: 10, y: 20 };
let c = Color { r: 255, g: 128, b: 0 };

// int matches the int arm
print(show(42 as dyn));
// Point implements Describable → matches interface arm
print(show(p as dyn));
// Color implements Describable → matches interface arm
print(show(c as dyn));
// string does not implement Describable → falls to default
print(show("hello" as dyn));
