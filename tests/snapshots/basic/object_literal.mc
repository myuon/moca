// Test map operations (migrated from object literal test)

// String key map with string values
let person = new Map<string, string> {};
person["name"] = "Alice";
person["city"] = "Tokyo";
print(person["name"]);
print(person["city"]);

// Int key map demonstrating computed keys
let x = 10;
let y = 20;
let point = new Map<int, string> {};
point[x] = "ten";
point[y] = "twenty";
point[x + y] = "thirty";
print(point[30]);
