# Moca development task runner

# Default task: run all checks
default: check

# Run all checks (fmt, clippy, test, moca-lint)
check: fmt-check clippy test moca-lint
    @echo "All checks passed!"

# Format check (doesn't modify files)
fmt-check:
    cargo fmt -- --check

# Format code
fmt:
    cargo fmt

# Auto-fix code issues and format
fix:
    cargo fix --allow-dirty --allow-staged
    cargo fmt

# Run clippy lints
clippy:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test

# Run tests with coverage report
coverage:
    cargo llvm-cov

# Check coverage meets minimum threshold (75% for compiler modules)
coverage-check:
    #!/usr/bin/env bash
    set -e
    echo "Checking coverage for compiler modules..."
    cargo llvm-cov --no-report > /dev/null 2>&1

    # Get coverage report and check each module
    report=$(cargo llvm-cov report 2>&1)

    failed=0
    for module in lexer parser codegen resolver typechecker types ast; do
        line=$(echo "$report" | grep "compiler/${module}.rs" || true)
        if [ -n "$line" ]; then
            # Extract line coverage percentage (last percentage in the line)
            coverage=$(echo "$line" | awk '{for(i=1;i<=NF;i++) if($i ~ /%/) last=$i} END{print last}' | tr -d '%')
            if [ -n "$coverage" ]; then
                threshold=75
                if (( $(echo "$coverage < $threshold" | bc -l) )); then
                    echo "FAIL: ${module}.rs coverage ${coverage}% < ${threshold}%"
                    failed=1
                else
                    echo "OK: ${module}.rs coverage ${coverage}%"
                fi
            fi
        fi
    done

    if [ $failed -eq 1 ]; then
        echo "Coverage check failed!"
        exit 1
    fi
    echo "Coverage check passed!"

# Run moca lint on std .mc files
moca-lint: build
    #!/usr/bin/env bash
    set -e
    failed=0
    # Known acceptable warnings (false positives):
    # - prelude.mc: Map.set() must call .put() internally (using []= would cause infinite recursion)
    known_warnings="use \`\\[\\] =\` indexing instead of \`\\.put\\(\\)\`"
    for file in std/*.mc; do
        output=$(./target/debug/moca lint "$file" 2>&1) || {
            unexpected=$(echo "$output" | grep -v -E "$known_warnings" | grep "^warning:" || true)
            if [ -n "$unexpected" ]; then
                echo "$output"
                failed=1
            fi
        }
    done
    if [ $failed -eq 1 ]; then
        echo "moca lint failed!"
        exit 1
    fi
    echo "moca lint passed!"

# Build the project
build:
    cargo build

# Build release
build-release:
    cargo build --release

# Clean build artifacts
clean:
    cargo clean

# Run a .mc file
run file:
    cargo run -- run {{file}}

# Watch for changes and run tests
watch:
    cargo watch -x test

# Generate full coverage report (HTML)
coverage-html:
    cargo llvm-cov --html
    @echo "Coverage report generated at target/llvm-cov/html/index.html"
