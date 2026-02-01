// Test map operations (migrated from object literal test)

// String key map with string values
let person: HashMapAny = map_new();
person.put_string("name", "Alice");
person.put_string("city", "Tokyo");
print(person.get_string("name"));
print(person.get_string("city"));

// Int key map demonstrating computed keys
let x = 10;
let y = 20;
let point: HashMapAny = map_new();
point.put_int(x, "ten");
point.put_int(y, "twenty");
point.put_int(x + y, "thirty");
print(point.get_int(30));
