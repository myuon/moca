// Map iteration test - keys and values methods

let m: map<any, any> = map::`new`();

// Add some entries
m.put(1, 100);
m.put(2, 200);
m.put(3, 300);

// Get keys and values
let keys: vec<any> = m.keys();
let values: vec<any> = m.values();

// Check counts
print(keys.len());
print(values.len());

// Sum all keys and values to verify content (order is not guaranteed)
var key_sum = 0;
var i = 0;
while i < keys.len() {
    key_sum = key_sum + keys.get(i);
    i = i + 1;
}
print(key_sum);

var value_sum = 0;
i = 0;
while i < values.len() {
    value_sum = value_sum + values.get(i);
    i = i + 1;
}
print(value_sum);
