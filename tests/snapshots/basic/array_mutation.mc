// Test fixed array mutation (indexing)
var arr = [10, 20, 30];
arr[1] = 25;
print(arr[1]);

// Test Vector operations (push/pop)
var vec = vec_new();
vec_push(vec, 10);
vec_push(vec, 20);
vec_push(vec, 30);
vec_push(vec, 40);
print(vec_len(vec));
print(vec[3]);
let last = vec_pop(vec);
print(last);
print(vec_len(vec));
