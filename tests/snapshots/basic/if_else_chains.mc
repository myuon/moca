// Test if-else chains
fun grade(score: int) -> string {
    if score >= 90 {
        return "A";
    } else {
        if score >= 80 {
            return "B";
        } else {
            if score >= 70 {
                return "C";
            } else {
                if score >= 60 {
                    return "D";
                } else {
                    return "F";
                }
            }
        }
    }
}

print(grade(95));
print(grade(85));
print(grade(75));
print(grade(65));
print(grade(55));

// Simple if-else
fun abs_value(x: int) -> int {
    if x < 0 {
        return 0 - x;
    } else {
        return x;
    }
}

print(abs_value(10));
print(abs_value(-5));
print(abs_value(0));
