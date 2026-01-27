// Test while loops
var counter = 0;
while counter < 5 {
    print(counter);
    counter = counter + 1;
}

// Nested while
var i = 0;
while i < 3 {
    var j = 0;
    while j < 2 {
        print(i * 10 + j);
        j = j + 1;
    }
    i = i + 1;
}

// While with break condition inside
var sum = 0;
var n = 1;
while n <= 10 {
    sum = sum + n;
    n = n + 1;
}
print(sum);
