// Map iteration test - keys and values methods

let m: HashMapAny = map_new();

// Add some entries
m.put_int(1, 100);
m.put_int(2, 200);
m.put_int(3, 300);

// Get keys and values
let keys: VectorAny = m.keys();
let values: VectorAny = m.values();

// Check counts
print(keys.len);
print(values.len);

// Sum all keys and values to verify content (order is not guaranteed)
var key_sum = 0;
var i = 0;
while i < keys.len {
    key_sum = key_sum + keys.get(i);
    i = i + 1;
}
print(key_sum);

var value_sum = 0;
i = 0;
while i < values.len {
    value_sum = value_sum + values.get(i);
    i = i + 1;
}
print(value_sum);
