// Map iteration test - keys and values methods

let m: Map<int, int> = Map<int, int>::`new`();

// Add some entries
m.put(1, 100);
m.put(2, 200);
m.put(3, 300);

// Get keys and values
let keys: Vec<int> = m.keys();
let values: Vec<int> = m.values();

// Check counts
print(keys.len());
print(values.len());

// Sum all keys and values to verify content (order is not guaranteed)
let key_sum = 0;
let i = 0;
while i < keys.len() {
    key_sum = key_sum + keys.get(i);
    i = i + 1;
}
print(key_sum);

let value_sum = 0;
i = 0;
while i < values.len() {
    value_sum = value_sum + values.get(i);
    i = i + 1;
}
print(value_sum);
