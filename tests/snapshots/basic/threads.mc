fun worker() {
    let sum = 0;
    let i = 0;
    while i < 100 {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

let handle = spawn(worker);
let result = join(handle);
print(result);
