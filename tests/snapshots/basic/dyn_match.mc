// Test dyn type and match dyn

// Basic as dyn boxing
let d1 = 42 as dyn;
let d2 = "hello" as dyn;
let d3 = true as dyn;
let d4 = 3.14 as dyn;

// match dyn with int
match dyn d1 {
    v: int => {
        print("int: " + v.to_string());
    }
    v: string => {
        print("string: " + v);
    }
    _ => {
        print("other");
    }
}

// match dyn with string
match dyn d2 {
    v: int => {
        print("int: " + v.to_string());
    }
    v: string => {
        print("string: " + v);
    }
    _ => {
        print("other");
    }
}

// match dyn with bool
match dyn d3 {
    v: int => {
        print("int: " + v.to_string());
    }
    v: bool => {
        print("bool: " + v.to_string());
    }
    _ => {
        print("other");
    }
}

// match dyn with float
match dyn d4 {
    v: int => {
        print("int: " + v.to_string());
    }
    v: float => {
        print("float: " + v.to_string());
    }
    _ => {
        print("other");
    }
}

// match dyn hitting default branch
match dyn d1 {
    v: string => {
        print("string: " + v);
    }
    _ => {
        print("default branch");
    }
}

// Function that takes dyn and dispatches
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

print(describe(42 as dyn));
print(describe("world" as dyn));
print(describe(false as dyn));
print(describe(2.718 as dyn));
