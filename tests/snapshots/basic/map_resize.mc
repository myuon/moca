// Map resize test - add more than initial capacity (16) entries

let m: HashMapAny = map_new();

// Add 20 entries to trigger resize (load factor > 0.75 = 12 entries)
var i = 0;
while i < 20 {
    m.put_int(i, i * 10);
    i = i + 1;
}

print(m.hm_size);

// Verify all entries are still accessible after resize
var all_ok = true;
i = 0;
while i < 20 {
    let val = m.get_int(i);
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
    if !m.contains_int(i) {
        contains_ok = false;
    }
    i = i + 1;
}

if contains_ok {
    print("all contains ok");
}

// Remove some entries
m.remove_int(5);
m.remove_int(10);
m.remove_int(15);
print(m.hm_size);

// Verify removed entries are gone
if !m.contains_int(5) && !m.contains_int(10) && !m.contains_int(15) {
    print("removed entries gone");
}
