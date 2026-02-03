// Benchmark: 500x500 nested loop
fun nested_loop() {
    var sum = 0;
    var i = 0;
    while i < 500 {
        var j = 0;
        while j < 500 {
            sum = sum + i * j;
            j = j + 1;
        }
        i = i + 1;
    }
    print(sum);
}

nested_loop();
