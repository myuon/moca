// Range loop variable unused — should warn
for x in 0..10 {
    print("hello");
}

// Underscore prefix — no warning
for _i in 0..10 {
    print("hello");
}

// Used variable — no warning
for i in 0..5 {
    print($"{i}");
}
