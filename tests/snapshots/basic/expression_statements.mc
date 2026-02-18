// Test expression statements
fun side_effect(x: int) -> int {
    print($"{x}");
    return x + 1;
}

// Expression statement (discarded result)
side_effect(1);
side_effect(2);

// Used result
let result = side_effect(3);
print($"{result}");
