// Basic exclusive range: 0..5
for i in 0..5 {
    print($"{i}");
}

// Inclusive range: 0..=3
for i in 0..=3 {
    print($"{i}");
}

// Range with expressions
let start = 2;
let end = 6;
for i in start..end {
    print($"{i}");
}

// Sum using range
let sum = 0;
for i in 1..=10 {
    sum = sum + i;
}
print($"{sum}");

// Empty range (start == end, exclusive)
for _i in 5..5 {
    print("should not print");
}

// Single element range (inclusive, start == end)
for i in 5..=5 {
    print($"{i}");
}
