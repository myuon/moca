fun fizzbuzz(n) {
    let i = 1;
    while i <= n {
        if i % 15 == 0 {
            print($"{-3}");
        } else if i % 3 == 0 {
            print($"{-1}");
        } else if i % 5 == 0 {
            print($"{-2}");
        } else {
            print($"{i}");
        }
        i = i + 1;
    }
}

fizzbuzz(15);
