// Benchmark: 1000x1000 nested loop
fun nested_loop() {
    var sum = 0;
    var i = 0;
    while i < 1000 {
        var j = 0;
        while j < 1000 {
            sum = sum + i * j;
            j = j + 1;
        }
        i = i + 1;
    }
    print(sum);
}

nested_loop();
