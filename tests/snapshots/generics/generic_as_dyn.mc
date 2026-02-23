// Test: TypeDesc from generic as dyn matches correctly in match dyn
fun box_it<T>(v: T) -> dyn {
    return v as dyn;
}

fun type_name_of(v: dyn) -> string {
    match dyn v {
        x: int => { return "int"; }
        x: float => { return "float"; }
        x: bool => { return "bool"; }
        x: string => { return "string"; }
        _ => { return "other"; }
    }
}

// Generic boxing should produce TypeDesc that matches in match dyn
print_str(type_name_of(box_it("hello")));
print_str("\n");
print_str(type_name_of(box_it(42)));
print_str("\n");
print_str(type_name_of(box_it(true)));
print_str("\n");
print_str(type_name_of(box_it(3.14)));
print_str("\n");

// Direct boxing should also work (regression check)
print_str(type_name_of("world"));
print_str("\n");
print_str(type_name_of(99));
print_str("\n");
