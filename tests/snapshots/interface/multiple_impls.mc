// Multiple types implementing the same interface

interface Printable {
    fun display(self) -> string;
}

struct Point { x: int, y: int }
struct Rect { w: int, h: int }

impl Printable for int {
    fun display(self) -> string {
        return "int:" + self.to_string();
    }
}

impl Printable for Point {
    fun display(self) -> string {
        return "Point(" + self.x.to_string() + "," + self.y.to_string() + ")";
    }
}

impl Printable for Rect {
    fun display(self) -> string {
        return "Rect(" + self.w.to_string() + "x" + self.h.to_string() + ")";
    }
}

fun show<T: Printable>(v: T) {
    print(v.display());
}

show<int>(42);
show<Point>(Point { x: 10, y: 20 });
show<Rect>(Rect { w: 100, h: 50 });
