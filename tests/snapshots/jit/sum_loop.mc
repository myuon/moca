fun sum_to(n) {
    var total = 0;
    var i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

print(sum_to(100));
print(sum_to(1000));
