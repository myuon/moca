// Test map operations (migrated from object literal test)

// String key map with string values
let person: map<any, any> = map_new();
person.put("name", "Alice");
person.put("city", "Tokyo");
print(person.get("name"));
print(person.get("city"));

// Int key map demonstrating computed keys
let x = 10;
let y = 20;
let point: map<any, any> = map_new();
point.put(x, "ten");
point.put(y, "twenty");
point.put(x + y, "thirty");
print(point.get(30));
