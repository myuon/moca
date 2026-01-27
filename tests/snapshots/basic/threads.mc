fun worker() {
    var sum = 0;
    var i = 0;
    while i < 100 {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

let handle = spawn(worker);
let result = join(handle);
print(result);
