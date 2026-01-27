//! In-process integration tests that contribute to coverage.
//!
//! These tests call the compiler/VM APIs directly instead of spawning
//! a separate process, so they are included in coverage measurement.
//!
//! Note: File-based snapshot tests are now in snapshot_tests.rs and also
//! run in-process for coverage.

// Direct compiler API tests for coverage
mod compiler_api_tests {
    use moca::compiler::{Codegen, Lexer, Parser, Resolver, TypeChecker};
    use moca::vm::VM;

    fn compile_and_run(source: &str) -> Result<(), String> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mc", tokens);
        let program = parser.parse()?;
        let mut typechecker = TypeChecker::new("test.mc");
        typechecker.check_program(&program).map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        let mut resolver = Resolver::new("test.mc");
        let resolved = resolver.resolve(program)?;
        let mut codegen = Codegen::new();
        let chunk = codegen.compile(resolved)?;
        let mut vm = VM::new();
        vm.run(&chunk)
    }

    fn compile_expect_error(source: &str) -> String {
        compile_and_run(source).expect_err("expected error")
    }

    // Nullable type tests
    #[test]
    fn test_nullable_type() {
        compile_and_run(
            r#"
            let x: int? = nil;
            let y: int? = 42;
            print(x);
            print(y);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_nullable_string() {
        compile_and_run(
            r#"
            let s: string? = nil;
            let t: string? = "hello";
            print(s);
            print(t);
        "#,
        )
        .unwrap();
    }

    // Object type tests
    #[test]
    fn test_nested_object() {
        compile_and_run(
            r#"
            let obj = { a: { b: 1 } };
            print(obj.a.b);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_object_with_array() {
        compile_and_run(
            r#"
            let obj = { items: [1, 2, 3] };
            print(obj.items[0]);
            print(obj.items[1]);
        "#,
        )
        .unwrap();
    }

    // Array type tests
    #[test]
    fn test_array_of_objects() {
        compile_and_run(
            r#"
            let arr = [{ x: 1 }, { x: 2 }];
            print(arr[0].x);
            print(arr[1].x);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_empty_array() {
        compile_and_run(
            r#"
            let arr: array<int> = [];
            print(arr);
        "#,
        )
        .unwrap();
    }

    // Function type tests
    #[test]
    fn test_function_call_chain() {
        compile_and_run(
            r#"
            fun add(a: int, b: int) -> int {
                return a + b;
            }
            fun mul(a: int, b: int) -> int {
                return a * b;
            }
            print(add(mul(2, 3), 4));
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_recursive_function() {
        compile_and_run(
            r#"
            fun sum(n: int) -> int {
                if n <= 0 {
                    return 0;
                }
                return n + sum(n - 1);
            }
            print(sum(5));
        "#,
        )
        .unwrap();
    }

    // Struct tests
    #[test]
    fn test_struct_with_array_field() {
        compile_and_run(
            r#"
            struct Container {
                items: array<int>
            }
            let c = Container { items: [1, 2, 3] };
            print(c.items[0]);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_struct_multiple() {
        compile_and_run(
            r#"
            struct Point { x: int, y: int }
            struct Size { width: int, height: int }
            let p = Point { x: 10, y: 20 };
            let s = Size { width: 100, height: 200 };
            print(p.x);
            print(s.width);
        "#,
        )
        .unwrap();
    }

    // Type error tests for typechecker coverage
    #[test]
    fn test_type_error_array_element() {
        let err = compile_expect_error(
            r#"
            let arr: array<int> = [1, "hello", 3];
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_type_error_function_return() {
        let err = compile_expect_error(
            r#"
            fun get_int() -> int {
                return "hello";
            }
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_type_error_function_argument() {
        let err = compile_expect_error(
            r#"
            fun takes_int(x: int) {
                print(x);
            }
            takes_int("hello");
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_type_error_binary_op() {
        let err = compile_expect_error(
            r#"
            let x = "hello" - 1;
        "#,
        );
        assert!(err.contains("type") || err.contains("string") || err.contains("int"));
    }

    #[test]
    fn test_type_error_if_condition() {
        let err = compile_expect_error(
            r#"
            if "not a bool" {
                print("yes");
            }
        "#,
        );
        assert!(err.contains("bool") || err.contains("expected") || err.contains("string"));
    }

    #[test]
    fn test_type_error_while_condition() {
        let err = compile_expect_error(
            r#"
            while 42 {
                print("loop");
            }
        "#,
        );
        assert!(err.contains("bool") || err.contains("expected") || err.contains("int"));
    }

    // Resolver error tests
    #[test]
    fn test_resolver_duplicate_function() {
        let err = compile_expect_error(
            r#"
            fun foo() { }
            fun foo() { }
        "#,
        );
        assert!(err.contains("foo") || err.contains("duplicate") || err.contains("already"));
    }

    #[test]
    fn test_resolver_duplicate_struct() {
        let err = compile_expect_error(
            r#"
            struct Point { x: int }
            struct Point { y: int }
        "#,
        );
        assert!(err.contains("Point") || err.contains("duplicate") || err.contains("already"));
    }

    // Complex expression tests
    #[test]
    fn test_complex_expression() {
        compile_and_run(
            r#"
            let x = (1 + 2) * 3 - 4 / 2;
            print(x);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_comparison_operators() {
        compile_and_run(
            r#"
            let a = 1;
            let b = 2;
            let c = 3;
            print(a < b);
            print(b < c);
            print(a <= b);
            print(b >= a);
            print(a == 1);
            print(b != 1);
        "#,
        )
        .unwrap();
    }

    // Float operations
    #[test]
    fn test_float_arithmetic() {
        compile_and_run(
            r#"
            let x = 3.14;
            let y = 2.0;
            print(x + y);
            print(x - y);
            print(x * y);
            print(x / y);
        "#,
        )
        .unwrap();
    }

    // String operations
    #[test]
    fn test_string_concatenation() {
        compile_and_run(
            r#"
            let s = "Hello, " + "World!";
            print(s);
        "#,
        )
        .unwrap();
    }

    // Control flow tests
    #[test]
    fn test_nested_if() {
        compile_and_run(
            r#"
            let x = 10;
            if x > 5 {
                if x > 8 {
                    print("big");
                } else {
                    print("medium");
                }
            } else {
                print("small");
            }
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_while_with_condition() {
        compile_and_run(
            r#"
            var i = 0;
            while i < 5 {
                print(i);
                i = i + 1;
            }
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_nested_while() {
        compile_and_run(
            r#"
            var i = 0;
            while i < 3 {
                var j = 0;
                while j < 2 {
                    print(i * 10 + j);
                    j = j + 1;
                }
                i = i + 1;
            }
        "#,
        )
        .unwrap();
    }

    // For-in tests
    #[test]
    fn test_for_in_range() {
        compile_and_run(
            r#"
            for i in [1, 2, 3, 4, 5] {
                print(i);
            }
        "#,
        )
        .unwrap();
    }

    // Return tests
    #[test]
    fn test_early_return() {
        compile_and_run(
            r#"
            fun check(x: int) -> int {
                if x < 0 {
                    return -1;
                }
                if x == 0 {
                    return 0;
                }
                return 1;
            }
            print(check(-5));
            print(check(0));
            print(check(5));
        "#,
        )
        .unwrap();
    }

    // Recursion tests
    #[test]
    fn test_mutual_recursion() {
        compile_and_run(
            r#"
            fun is_even(n: int) -> bool {
                if n == 0 { return true; }
                return is_odd(n - 1);
            }
            fun is_odd(n: int) -> bool {
                if n == 0 { return false; }
                return is_even(n - 1);
            }
            print(is_even(4));
            print(is_odd(4));
        "#,
        )
        .unwrap();
    }

    // Additional type error tests for better typechecker coverage
    #[test]
    fn test_type_error_object_field() {
        let err = compile_expect_error(
            r#"
            let obj = { x: 1, y: "hello" };
            let z: int = obj.y;
        "#,
        );
        assert!(err.contains("expected") || err.contains("string") || err.contains("int"));
    }

    #[test]
    fn test_type_error_array_index_type() {
        let err = compile_expect_error(
            r#"
            let arr = [1, 2, 3];
            let x = arr["bad"];
        "#,
        );
        assert!(err.contains("int") || err.contains("string") || err.contains("index"));
    }

    #[test]
    fn test_type_error_struct_field_type() {
        let err = compile_expect_error(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: "bad" };
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_type_error_unary_not_int() {
        let err = compile_expect_error(
            r#"
            let x = !42;
        "#,
        );
        assert!(err.contains("bool") || err.contains("int") || err.contains("expected"));
    }

    #[test]
    fn test_type_error_unary_neg_bool() {
        let err = compile_expect_error(
            r#"
            let x = -true;
        "#,
        );
        assert!(err.contains("int") || err.contains("float") || err.contains("bool"));
    }

    #[test]
    fn test_type_error_for_in_non_array() {
        let err = compile_expect_error(
            r#"
            for i in 42 {
                print(i);
            }
        "#,
        );
        assert!(err.contains("array") || err.contains("int") || err.contains("expected"));
    }

    #[test]
    fn test_type_error_call_non_function() {
        let err = compile_expect_error(
            r#"
            let x = 42;
            x();
        "#,
        );
        assert!(err.contains("function") || err.contains("call") || err.contains("int"));
    }

    #[test]
    fn test_type_error_wrong_arg_count() {
        let err = compile_expect_error(
            r#"
            fun add(a: int, b: int) -> int {
                return a + b;
            }
            add(1);
        "#,
        );
        assert!(err.contains("argument") || err.contains("2") || err.contains("1"));
    }

    // Parser error tests
    #[test]
    fn test_parse_error_unclosed_paren() {
        let err = compile_expect_error(
            r#"
            let x = (1 + 2;
        "#,
        );
        assert!(err.contains("RParen") || err.contains(")") || err.contains("expected"));
    }

    #[test]
    fn test_parse_error_unclosed_brace() {
        let err = compile_expect_error(
            r#"
            fun test() {
                print(1);
        "#,
        );
        assert!(err.contains("RBrace") || err.contains("}") || err.contains("expected"));
    }

    #[test]
    fn test_parse_error_unclosed_bracket() {
        let err = compile_expect_error(
            r#"
            let arr = [1, 2, 3;
        "#,
        );
        assert!(err.contains("RBracket") || err.contains("]") || err.contains("expected"));
    }

    #[test]
    fn test_parse_error_missing_semicolon() {
        let err = compile_expect_error(
            r#"
            let x = 1
            let y = 2;
        "#,
        );
        assert!(err.contains("Semicolon") || err.contains(";") || err.contains("expected"));
    }

    // Resolver error tests
    #[test]
    fn test_resolver_undefined_in_expr() {
        let err = compile_expect_error(
            r#"
            let x = y + 1;
        "#,
        );
        assert!(err.contains("undefined") || err.contains("y"));
    }

    #[test]
    fn test_resolver_assign_to_undefined() {
        let err = compile_expect_error(
            r#"
            x = 42;
        "#,
        );
        assert!(err.contains("undefined") || err.contains("x"));
    }

    // Additional coverage tests
    #[test]
    fn test_empty_function() {
        compile_and_run(
            r#"
            fun empty() {
            }
            empty();
            print("done");
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_function_no_return_type() {
        compile_and_run(
            r#"
            fun greet(name: string) {
                print("Hello, ");
                print(name);
            }
            greet("World");
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_nested_array_access() {
        compile_and_run(
            r#"
            let arr = [[1, 2], [3, 4], [5, 6]];
            print(arr[0][0]);
            print(arr[1][1]);
            print(arr[2][0]);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_object_empty() {
        compile_and_run(
            r#"
            let obj = {};
            print(obj);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_struct_field_update() {
        compile_and_run(
            r#"
            struct Counter { value: int }
            var c = Counter { value: 0 };
            c.value = c.value + 1;
            print(c.value);
            c.value = c.value + 1;
            print(c.value);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_type_annotation_function_param() {
        compile_and_run(
            r#"
            fun process(items: array<int>) -> int {
                var sum = 0;
                for item in items {
                    sum = sum + item;
                }
                return sum;
            }
            print(process([1, 2, 3, 4, 5]));
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_object_type_annotation() {
        compile_and_run(
            r#"
            let obj: {x: int, y: int} = { x: 10, y: 20 };
            print(obj.x);
            print(obj.y);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_multiple_return_paths() {
        compile_and_run(
            r#"
            fun classify(n: int) -> string {
                if n < 0 {
                    return "negative";
                }
                if n == 0 {
                    return "zero";
                }
                return "positive";
            }
            print(classify(-5));
            print(classify(0));
            print(classify(5));
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_deeply_nested_expression() {
        compile_and_run(
            r#"
            let x = ((((1 + 2) * 3) - 4) / 2);
            print(x);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_mixed_arithmetic() {
        compile_and_run(
            r#"
            let a = 10;
            let b = 3;
            print(a + b);
            print(a - b);
            print(a * b);
            print(a / b);
            print(a % b);
        "#,
        )
        .unwrap();
    }

    // Tests for ast.rs Expr::span() coverage (Object, StructLiteral patterns)
    #[test]
    fn test_type_error_in_object_literal() {
        // Error occurs in object literal, triggering Expr::Object span()
        let err = compile_expect_error(
            r#"
            let obj: {x: int} = { x: "not an int" };
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_type_error_in_struct_literal() {
        // Error occurs in struct literal, triggering Expr::StructLiteral span()
        let err = compile_expect_error(
            r#"
            struct Point { x: int, y: int }
            let p: Point = Point { x: "bad", y: 2 };
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_object_field_type_inference() {
        // Test object type inference paths
        compile_and_run(
            r#"
            let obj = { name: "test", value: 42, flag: true };
            print(obj.name);
            print(obj.value);
            print(obj.flag);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_struct_literal_all_fields() {
        // Test struct literal with all field types
        compile_and_run(
            r#"
            struct Data {
                num: int,
                text: string,
                flag: bool
            }
            let d = Data { num: 100, text: "hello", flag: false };
            print(d.num);
            print(d.text);
            print(d.flag);
        "#,
        )
        .unwrap();
    }

    // Additional typechecker coverage tests
    #[test]
    fn test_type_error_object_missing_field() {
        let err = compile_expect_error(
            r#"
            let obj: {x: int, y: int} = { x: 1 };
        "#,
        );
        assert!(err.contains("field") || err.contains("y") || err.contains("expected"));
    }

    #[test]
    fn test_type_error_struct_missing_field() {
        let err = compile_expect_error(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1 };
        "#,
        );
        assert!(err.contains("field") || err.contains("y") || err.contains("expected"));
    }

    #[test]
    fn test_nullable_assignment() {
        compile_and_run(
            r#"
            var x: int? = 42;
            x = nil;
            print(x);
            x = 100;
            print(x);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_type_annotation_array_nested() {
        compile_and_run(
            r#"
            let arr: array<array<int>> = [[1, 2], [3, 4]];
            print(arr[0][0]);
            print(arr[1][1]);
        "#,
        )
        .unwrap();
    }

    // More typechecker coverage
    #[test]
    fn test_void_function_returns_nil() {
        compile_and_run(
            r#"
            fun test() {
                print("test");
            }
            test();
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_type_error_assign_wrong_type() {
        let err = compile_expect_error(
            r#"
            var x: int = 1;
            x = "hello";
        "#,
        );
        assert!(err.contains("expected") || err.contains("int") || err.contains("string"));
    }

    #[test]
    fn test_struct_with_nullable() {
        compile_and_run(
            r#"
            struct User {
                name: string,
                age: int?
            }
            let u1 = User { name: "Alice", age: 30 };
            let u2 = User { name: "Bob", age: nil };
            print(u1.name);
            print(u2.name);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_function_with_object_param() {
        compile_and_run(
            r#"
            fun get_x(obj: {x: int}) -> int {
                return obj.x;
            }
            let result = get_x({ x: 42 });
            print(result);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_type_inference_binary_ops() {
        compile_and_run(
            r#"
            let a = 1 + 2;
            let b = 1.0 + 2.0;
            let c = "a" + "b";
            let d = true;
            print(a);
            print(b);
            print(c);
            print(d);
        "#,
        )
        .unwrap();
    }

    #[test]
    fn test_type_error_comparison_mismatch() {
        let err = compile_expect_error(
            r#"
            let x = 1 < "hello";
        "#,
        );
        assert!(err.contains("type") || err.contains("int") || err.contains("string"));
    }
}
