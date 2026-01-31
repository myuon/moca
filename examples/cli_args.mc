// Example: CLI arguments demonstration
// Usage: moca run examples/cli_args.mc hello world 123

fun main() {
    print("argc: ");
    print(argc());

    print("argv(0): ");
    print(argv(0));

    var i = 1;
    while i < argc() {
        print("argv(" + to_string(i) + "): ");
        print(argv(i));
        i = i + 1;
    }

    print("args(): ");
    var all_args = args();
    var j = 0;
    while j < len(all_args) {
        print("  [" + to_string(j) + "] = " + all_args[j]);
        j = j + 1;
    }
}

main();
