// Test map operations (migrated from object literal test)

// String key map with string values
let person = map_new_any();
map_put_string(person, "name", "Alice");
map_put_string(person, "city", "Tokyo");
print(map_get_string(person, "name"));
print(map_get_string(person, "city"));

// Int key map demonstrating computed keys
let x = 10;
let y = 20;
let point = map_new_any();
map_put_int(point, x, "ten");
map_put_int(point, y, "twenty");
map_put_int(point, x + y, "thirty");
print(map_get_int(point, 30));
