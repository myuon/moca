// Test user-defined struct associated functions

struct Point {
    x: int,
    y: int
}

impl Point {
    // Associated function (no self parameter)
    fun origin() -> Point {
        return Point { x: 0, y: 0 };
    }

    // Associated function with arguments
    fun create(x: int, y: int) -> Point {
        return Point { x: x, y: y };
    }
}

// Call associated functions
let p1 = Point::origin();
print(p1.x);
print(p1.y);

let p2 = Point::create(3, 4);
print(p2.x);
print(p2.y);

// Another struct with associated function
struct Counter {
    value: int
}

impl Counter {
    fun zero() -> Counter {
        return Counter { value: 0 };
    }

    fun with_value(v: int) -> Counter {
        return Counter { value: v };
    }
}

let c = Counter::zero();
print(c.value);

let c2 = Counter::with_value(100);
print(c2.value);
