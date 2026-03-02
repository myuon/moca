// break in while
let i = 0;
while i < 10 {
    if i == 5 {
        break;
    }
    print(i);
    i = i + 1;
}

// continue in while
let j = 0;
while j < 10 {
    j = j + 1;
    if j % 2 == 0 {
        continue;
    }
    print(j);
}

// break in for-range
for k in 0..10 {
    if k == 3 {
        break;
    }
    print(k);
}

// continue in for-range
for m in 0..10 {
    if m % 2 == 0 {
        continue;
    }
    print(m);
}

// nested loops - break inner only
for i in 0..3 {
    for j in 0..3 {
        if j == 1 {
            break;
        }
        print(i * 10 + j);
    }
}
