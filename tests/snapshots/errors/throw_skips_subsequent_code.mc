// Test that code after throw is not executed

// This should print
print("before throw");

// Throw an error
throw "error message";

// This should NOT print
print("after throw - this should not print");
