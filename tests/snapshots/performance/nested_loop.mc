// Benchmark: 3000x3000 nested loop
fun nested_loop() {
    let sum = 0;
    let i = 0;
    while i < 3000 {
        let j = 0;
        while j < 3000 {
            sum = sum + i * j;
            j = j + 1;
        }
        i = i + 1;
    }
    print(sum);
}

nested_loop();
