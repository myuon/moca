// Explicit ToString impl takes priority over auto-derive

struct Color {
    r: int,
    g: int,
    b: int
}

impl ToString for Color {
    fun to_string(self) -> string {
        return "rgb(" + self.r.to_string() + ", " + self.g.to_string() + ", " + self.b.to_string() + ")";
    }
}

let c = Color { r: 255, g: 128, b: 0 };
print(c);
