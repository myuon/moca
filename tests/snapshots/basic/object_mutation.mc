// Test map mutation (migrated from object)
var obj: HashMapAny = map_new();
obj.put_string("value", 100);
print(obj.get_string("value"));
obj.put_string("value", 200);
print(obj.get_string("value"));
obj.put_string("newField", 300);
print(obj.get_string("newField"));
