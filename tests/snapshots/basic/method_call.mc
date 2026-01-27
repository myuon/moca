// Test method call implementation

// Simple struct with methods
struct Counter {
    count: int
}

impl Counter {
    fun get(self) -> int {
        return self.count;
    }

    fun increment(self) {
        self.count = self.count + 1;
    }

    fun add(self, n: int) {
        self.count = self.count + n;
    }

    fun reset(self) {
        self.count = 0;
    }
}

// Test 1: Method with return value (read self.field)
let c1 = Counter { count: 10 };
print(c1.get());

// Test 2: Method that modifies self
var c2 = Counter { count: 0 };
c2.increment();
print(c2.get());

c2.increment();
c2.increment();
print(c2.get());

// Test 3: Method with arguments
var c3 = Counter { count: 5 };
c3.add(10);
print(c3.get());

// Test 4: Multiple method calls
var c4 = Counter { count: 100 };
c4.add(50);
c4.increment();
print(c4.get());
c4.reset();
print(c4.get());

// Test 5: Struct with multiple fields
struct Point {
    x: int,
    y: int
}

impl Point {
    fun sum(self) -> int {
        return self.x + self.y;
    }

    fun set_x(self, val: int) {
        self.x = val;
    }
}

let p = Point { x: 3, y: 4 };
print(p.sum());

var p2 = Point { x: 0, y: 10 };
p2.set_x(5);
print(p2.sum());

// Test 6: Method chaining (method returns struct)
struct Builder {
    value: int
}

impl Builder {
    fun add(self, n: int) -> Builder {
        return Builder { value: self.value + n };
    }

    fun get(self) -> int {
        return self.value;
    }
}

let b = Builder { value: 0 };
let b2 = b.add(5);
let b3 = b2.add(10);
print(b3.get());
