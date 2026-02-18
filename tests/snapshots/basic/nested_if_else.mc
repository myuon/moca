// Test nested if-else chains
fun classify(n: int) -> string {
    if n < 0 {
        return "negative";
    } else {
        if n == 0 {
            return "zero";
        } else {
            if n < 10 {
                return "small";
            } else {
                return "large";
            }
        }
    }
}

print($"{classify(-5)}");
print($"{classify(0)}");
print($"{classify(5)}");
print($"{classify(100)}");
