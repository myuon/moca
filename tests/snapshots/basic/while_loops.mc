// Test while loops
let counter = 0;
while counter < 5 {
    print($"{counter}");
    counter = counter + 1;
}

// Nested while
let i = 0;
while i < 3 {
    let j = 0;
    while j < 2 {
        print($"{i * 10 + j}");
        j = j + 1;
    }
    i = i + 1;
}

// While with break condition inside
let sum = 0;
let n = 1;
while n <= 10 {
    sum = sum + n;
    n = n + 1;
}
print($"{sum}");
