// Map iteration test - map_keys and map_values

let m = map_new_any();

// Add some entries
map_put_int(m, 1, 100);
map_put_int(m, 2, 200);
map_put_int(m, 3, 300);

// Get keys and values
let keys = map_keys(m);
let values = map_values(m);

// Check counts
print(keys.len);
print("\n");
print(values.len);
print("\n");

// Sum all keys and values to verify content (order is not guaranteed)
var key_sum = 0;
var i = 0;
while i < keys.len {
    key_sum = key_sum + vec_get_any(keys, i);
    i = i + 1;
}
print(key_sum);
print("\n");

var value_sum = 0;
i = 0;
while i < values.len {
    value_sum = value_sum + vec_get_any(values, i);
    i = i + 1;
}
print(value_sum);
print("\n");
