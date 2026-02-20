// Basic interface definition, impl, and bounded generic function

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

print(show_greet<int>(42));
print(show_greet<string>("world"));
