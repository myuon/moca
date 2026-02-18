// Example: CLI arguments demonstration
// Usage: moca run examples/cli_args.mc hello world 123

fun main() {
    print("argc: ");
    print(argc());

    print("argv(0): ");
    print(argv(0));

    let i = 1;
    while i < argc() {
        print("argv(" + i.to_string() + "): ");
        print(argv(i));
        i = i + 1;
    }

    print("args(): ");
    let all_args = args();
    let j = 0;
    while j < len(all_args) {
        print("  [" + j.to_string() + "] = " + all_args[j]);
        j = j + 1;
    }
}

main();
