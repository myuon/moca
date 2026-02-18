// Benchmark: 8000x8000 nested loop
fun nested_loop() {
    let sum = 0;
    let i = 0;
    while i < 8000 {
        let j = 0;
        while j < 8000 {
            sum = sum + i * j;
            j = j + 1;
        }
        i = i + 1;
    }
    print($"{sum}");
}

nested_loop();
