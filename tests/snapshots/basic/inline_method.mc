// Test @inline on struct methods
struct Point {
    x: int,
    y: int
}

impl Point {
    @inline
    fun get_x(self) -> int {
        return self.x;
    }

    @inline
    fun get_y(self) -> int {
        return self.y;
    }

    @inline
    fun sum(self) -> int {
        return self.x + self.y;
    }
}

let p = Point { x: 10, y: 20 };
print($"{p.get_x()}");
print($"{p.get_y()}");
print($"{p.sum()}");
