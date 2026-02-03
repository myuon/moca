// Error: calling unknown associated function

struct Point {
    x: int,
    y: int
}

impl Point {
    fun origin() -> Point {
        return Point { x: 0, y: 0 };
    }
}

// Call non-existent associated function
let p = Point::unknown();
