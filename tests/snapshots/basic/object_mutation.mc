// Test map mutation (migrated from object)
let obj = new Map<string, int> {};
obj["value"] = 100;
print(obj["value"]);
obj["value"] = 200;
print(obj["value"]);
obj["newField"] = 300;
print(obj["newField"]);
