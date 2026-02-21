// Test print fallback: print(v) where v doesn't implement ToString
// should use dyn-based formatter instead of erroring

struct Point { x: int, y: int }

// print on struct without ToString — should use dyn fallback
print(Point { x: 1, y: 2 });

// print on primitives with ToString — should still use normal path
print(42);
print("hello");
print(true);
print(3.14);

// implicit dyn coercion: inspect takes dyn, no explicit "as dyn" needed
inspect(Point { x: 10, y: 20 });
inspect(99);
