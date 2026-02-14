fun sum_to(n) {
    let total = 0;
    let i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

print(sum_to(100));
print(sum_to(1000));
