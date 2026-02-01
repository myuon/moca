// Test fixed array mutation (indexing)
var arr = [10, 20, 30];
arr[1] = 25;
print(arr[1]);

// Test Vector operations (push/pop)
var vec: VectorAny = vec_new();
vec.push(10);
vec.push(20);
vec.push(30);
vec.push(40);
print(vec.len);
print(vec.get(3));
let last = vec.pop();
print(last);
print(vec.len);
