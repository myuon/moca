// Test match dyn with return in arms (requires arm type unification)

fun describe(d: dyn) -> string {
    match dyn d {
        v: int => {
            return "integer=" + v.to_string();
        }
        v: string => {
            return "string=" + v;
        }
        v: bool => {
            return "bool=" + v.to_string();
        }
        v: float => {
            return "float=" + v.to_string();
        }
        _ => {
            return "unknown";
        }
    }
}

print(describe(42));
print(describe("world"));
print(describe(false));
print(describe(2.718));
print(describe(nil));
