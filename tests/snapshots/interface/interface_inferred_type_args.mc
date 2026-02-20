// Test interface-bounded generic functions with inferred type arguments

interface Greet {
    fun greet(self) -> string;
}

impl Greet for int {
    fun greet(self) -> string {
        return "hello from int";
    }
}

impl Greet for string {
    fun greet(self) -> string {
        return "hello from string";
    }
}

fun show_greet<T: Greet>(v: T) -> string {
    return v.greet();
}

// Type arguments inferred (no explicit <int> or <string>)
print(show_greet(42));
print(show_greet("world"));
