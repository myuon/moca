// Map resize test - add more than initial capacity (16) entries

let m = map_new_any();

// Add 20 entries to trigger resize (load factor > 0.75 = 12 entries)
var i = 0;
while i < 20 {
    map_put_int(m, i, i * 10);
    i = i + 1;
}

print(map_len(m));

// Verify all entries are still accessible after resize
var all_ok = true;
i = 0;
while i < 20 {
    let val = map_get_int(m, i);
    if val != i * 10 {
        all_ok = false;
    }
    i = i + 1;
}

if all_ok {
    print("all entries ok");
}

// Test contains for all keys
var contains_ok = true;
i = 0;
while i < 20 {
    if !map_contains_int(m, i) {
        contains_ok = false;
    }
    i = i + 1;
}

if contains_ok {
    print("all contains ok");
}

// Remove some entries
map_remove_int(m, 5);
map_remove_int(m, 10);
map_remove_int(m, 15);
print(map_len(m));

// Verify removed entries are gone
if !map_contains_int(m, 5) && !map_contains_int(m, 10) && !map_contains_int(m, 15) {
    print("removed entries gone");
}
