// Error: impl missing a required method

interface Greetable {
    fun greet(self) -> string;
    fun farewell(self) -> string;
}

impl Greetable for int {
    fun greet(self) -> string {
        return "hi";
    }
}
