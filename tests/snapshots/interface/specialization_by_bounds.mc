// Test: function specialization by trait bounds
// When the same function name has overloads with different bounds,
// the most specific (most bounds) version is selected.

interface Describable {
    fun describe(self) -> string;
}

impl Describable for int {
    fun describe(self) -> string {
        return "int";
    }
}

impl Describable for string {
    fun describe(self) -> string {
        return "string:" + self;
    }
}

// Specialized version: for types implementing Describable
fun show<T: Describable>(v: T) -> string {
    return "specialized: " + v.describe();
}

// Fallback version: for types without Describable
fun show<T>(v: T) -> string {
    return "fallback";
}

// int implements Describable -> specialized version
print(show(42));
// string implements Describable -> specialized version
print(show("hello"));
// bool does NOT implement Describable -> fallback version
print(show(true));
