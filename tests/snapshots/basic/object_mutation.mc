// Test map mutation (migrated from object)
var obj: map<any, any> = map::new();
obj.put("value", 100);
print(obj.get("value"));
obj.put("value", 200);
print(obj.get("value"));
obj.put("newField", 300);
print(obj.get("newField"));
