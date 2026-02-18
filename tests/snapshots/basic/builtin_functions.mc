// type_of
print(type_of(42));
print(type_of(3.14));
print(type_of(true));
print(type_of(nil));
print(type_of("hello"));
print(type_of([1, 2, 3]));

// to_string
print(42.to_string());
print(3.14.to_string());
print(true.to_string());
print("nil");

// parse_int
let n = parse_int("42");
print($"{n}");
print($"{n + 8}");
