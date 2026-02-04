// Benchmark: sum of 1 to 10,000,000
fun sum_loop() {
    var sum = 0;
    var i = 1;
    while i <= 10000000 {
        sum = sum + i;
        i = i + 1;
    }
    print(sum);
}

sum_loop();
