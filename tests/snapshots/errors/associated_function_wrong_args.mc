// Error: Point::origin() expects 0 arguments

struct Point {
    x: int,
    y: int
}

impl Point {
    fun origin() -> Point {
        return Point { x: 0, y: 0 };
    }
}

// Call with wrong number of arguments
let p = Point::origin(1, 2);
