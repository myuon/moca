// Map resize test - add more than initial capacity (16) entries

let m: map<any, any> = map::new();

// Add 20 entries to trigger resize (load factor > 0.75 = 12 entries)
var i = 0;
while i < 20 {
    m.put(i, i * 10);
    i = i + 1;
}

print(m.len());

// Verify all entries are still accessible after resize
var all_ok = true;
i = 0;
while i < 20 {
    let val = m.get(i);
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
    if !m.contains(i) {
        contains_ok = false;
    }
    i = i + 1;
}

if contains_ok {
    print("all contains ok");
}

// Remove some entries
m.remove(5);
m.remove(10);
m.remove(15);
print(m.len());

// Verify removed entries are gone
if !m.contains(5) && !m.contains(10) && !m.contains(15) {
    print("removed entries gone");
}
