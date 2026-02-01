// Test map mutation (migrated from object)
var obj = map_new_any();
map_put_string(obj, "value", 100);
print(map_get_string(obj, "value"));
map_put_string(obj, "value", 200);
print(map_get_string(obj, "value"));
map_put_string(obj, "newField", 300);
print(map_get_string(obj, "newField"));
