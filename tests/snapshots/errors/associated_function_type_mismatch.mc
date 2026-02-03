// Error: associated function argument type mismatch

struct Point {
    x: int,
    y: int
}

impl Point {
    fun from_coords(x: int, y: int) -> Point {
        return Point { x: x, y: y };
    }
}

// Call with wrong argument types (string instead of int)
let p = Point::from_coords("hello", 2);
