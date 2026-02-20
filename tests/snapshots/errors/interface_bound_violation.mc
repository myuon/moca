// Error: calling bounded generic with type that doesn't implement interface

interface Showable {
    fun show(self) -> string;
}

impl Showable for int {
    fun show(self) -> string {
        return "int";
    }
}

fun display<T: Showable>(v: T) -> string {
    return v.show();
}

// string does not implement Showable
print(display<string>("hello"));
