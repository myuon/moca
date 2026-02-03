//! Mandelbrot set comparison tests.
//!
//! Compares the output of the moca implementation with a Rust implementation
//! to verify correctness and measure performance differences.

use std::path::Path;
use std::time::Instant;

use moca::compiler::run_file_capturing_output;
use moca::config::RuntimeConfig;

/// Character set for rendering (10 characters, index 0-9)
/// Space = inside the Mandelbrot set (never escapes)
/// @ = fastest divergence (escapes immediately)
const CHARS: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

/// Generate Mandelbrot set ASCII art using the same algorithm as the moca version.
///
/// # Arguments
/// * `max_iter` - Maximum number of iterations before considering a point in the set
///
/// # Returns
/// A string containing the 80x24 ASCII art representation
fn mandelbrot_rust(max_iter: i32) -> String {
    let width = 80;
    let height = 24;

    // Coordinate range: real [-2.0, 1.0], imag [-1.0, 1.0]
    let x_min = -2.0_f64;
    let x_max = 1.0_f64;
    let y_min = -1.0_f64;
    let y_max = 1.0_f64;

    let x_step = (x_max - x_min) / 80.0;
    let y_step = (y_max - y_min) / 24.0;

    let mut result = String::new();

    let mut cy = y_min;
    for _ in 0..height {
        let mut cx = x_min;
        for _ in 0..width {
            // Mandelbrot iteration: z = z^2 + c
            let mut zr = 0.0_f64;
            let mut zi = 0.0_f64;
            let mut iter = 0_i32;

            while iter < max_iter {
                let zr2 = zr * zr;
                let zi2 = zi * zi;

                // Check if |z| > 2 (escaped)
                if zr2 + zi2 > 4.0 {
                    // Mark as escaped by adding max_iter + 1 to iter
                    iter = max_iter + iter + 1;
                }

                if iter < max_iter {
                    // z = z^2 + c
                    let new_zr = zr2 - zi2 + cx;
                    let new_zi = 2.0 * zr * zi + cy;
                    zr = new_zr;
                    zi = new_zi;
                    iter += 1;
                }
            }

            // Select character based on iteration count
            let mut char_idx = 0_i32;
            if iter > max_iter {
                // Escaped: map iteration count to character index (0-9)
                let escaped_iter = iter - max_iter - 1;
                char_idx = escaped_iter * 9 / max_iter;
                if char_idx > 9 {
                    char_idx = 9;
                }
            }
            // If iter == max_iter, point is in the set (char_idx = 0, space)

            result.push(CHARS[char_idx as usize]);
            cx += x_step;
        }

        result.push('\n');
        cy += y_step;
    }

    result
}

/// Generate moca source code for mandelbrot with given max_iter
fn generate_moca_code(max_iter: i32) -> String {
    let mut code = String::new();
    code.push_str("// Mandelbrot set ASCII art generator\n\n");
    code.push_str("fun print_char(idx: int) {\n");
    code.push_str("    if idx == 0 { print_str(\" \"); return; }\n");
    code.push_str("    if idx == 1 { print_str(\".\"); return; }\n");
    code.push_str("    if idx == 2 { print_str(\":\"); return; }\n");
    code.push_str("    if idx == 3 { print_str(\"-\"); return; }\n");
    code.push_str("    if idx == 4 { print_str(\"=\"); return; }\n");
    code.push_str("    if idx == 5 { print_str(\"+\"); return; }\n");
    code.push_str("    if idx == 6 { print_str(\"*\"); return; }\n");
    code.push_str("    if idx == 7 { print_str(\"#\"); return; }\n");
    code.push_str("    if idx == 8 { print_str(\"%\"); return; }\n");
    code.push_str("    print_str(\"@\");\n");
    code.push_str("}\n\n");
    code.push_str("fun mandelbrot(max_iter: int) {\n");
    code.push_str("    let width = 80;\n");
    code.push_str("    let height = 24;\n\n");
    code.push_str("    let x_min = -2.0;\n");
    code.push_str("    let x_max = 1.0;\n");
    code.push_str("    let y_min = -1.0;\n");
    code.push_str("    let y_max = 1.0;\n\n");
    code.push_str("    let x_step = (x_max - x_min) / 80.0;\n");
    code.push_str("    let y_step = (y_max - y_min) / 24.0;\n\n");
    code.push_str("    var cy = y_min;\n");
    code.push_str("    var py = 0;\n");
    code.push_str("    while py < height {\n");
    code.push_str("        var cx = x_min;\n");
    code.push_str("        var px = 0;\n");
    code.push_str("        while px < width {\n");
    code.push_str("            var zr = 0.0;\n");
    code.push_str("            var zi = 0.0;\n");
    code.push_str("            var iter = 0;\n\n");
    code.push_str("            while iter < max_iter {\n");
    code.push_str("                let zr2 = zr * zr;\n");
    code.push_str("                let zi2 = zi * zi;\n\n");
    code.push_str("                if zr2 + zi2 > 4.0 {\n");
    code.push_str("                    iter = max_iter + iter + 1;\n");
    code.push_str("                }\n\n");
    code.push_str("                if iter < max_iter {\n");
    code.push_str("                    let new_zr = zr2 - zi2 + cx;\n");
    code.push_str("                    let new_zi = 2.0 * zr * zi + cy;\n");
    code.push_str("                    zr = new_zr;\n");
    code.push_str("                    zi = new_zi;\n");
    code.push_str("                    iter = iter + 1;\n");
    code.push_str("                }\n");
    code.push_str("            }\n\n");
    code.push_str("            var char_idx = 0;\n");
    code.push_str("            if iter > max_iter {\n");
    code.push_str("                let escaped_iter = iter - max_iter - 1;\n");
    code.push_str("                char_idx = escaped_iter * 9 / max_iter;\n");
    code.push_str("                if char_idx > 9 {\n");
    code.push_str("                    char_idx = 9;\n");
    code.push_str("                }\n");
    code.push_str("            }\n\n");
    code.push_str("            print_char(char_idx);\n");
    code.push_str("            cx = cx + x_step;\n");
    code.push_str("            px = px + 1;\n");
    code.push_str("        }\n\n");
    code.push_str("        print_str(\"\\n\");\n");
    code.push_str("        cy = cy + y_step;\n");
    code.push_str("        py = py + 1;\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
    code.push_str(&format!("mandelbrot({});\n", max_iter));
    code
}

/// Run the moca mandelbrot implementation and return its output.
fn run_moca_mandelbrot(max_iter: i32) -> String {
    let moca_code = generate_moca_code(max_iter);

    // Write to a temporary file with unique name based on max_iter and process id
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "mandelbrot_test_{}_{}.mc",
        max_iter,
        std::process::id()
    ));
    std::fs::write(&temp_file, &moca_code).expect("Failed to write temp file");

    // Run the moca file
    let config = RuntimeConfig::default();
    let (output, result) = run_file_capturing_output(&temp_file, &config);

    // Clean up
    let _ = std::fs::remove_file(&temp_file);

    match result {
        Ok(()) => output.stdout,
        Err(e) => panic!("Moca execution failed: {}", e),
    }
}

/// Compare moca and Rust implementations with timing information.
fn compare_mandelbrot(max_iter: i32) {
    println!("\n=== Mandelbrot Comparison (max_iter={}) ===", max_iter);

    // Run Rust version with timing
    let rust_start = Instant::now();
    let rust_output = mandelbrot_rust(max_iter);
    let rust_duration = rust_start.elapsed();

    // Run moca version with timing
    let moca_start = Instant::now();
    let moca_output = run_moca_mandelbrot(max_iter);
    let moca_duration = moca_start.elapsed();

    // Print timing information
    println!("Rust execution time: {:?}", rust_duration);
    println!("Moca execution time: {:?}", moca_duration);
    println!(
        "Time difference: {:?} (Rust - Moca)",
        if rust_duration > moca_duration {
            rust_duration - moca_duration
        } else {
            moca_duration - rust_duration
        }
    );
    println!(
        "Moca is {:.2}x {} than Rust",
        if moca_duration > rust_duration {
            moca_duration.as_secs_f64() / rust_duration.as_secs_f64()
        } else {
            rust_duration.as_secs_f64() / moca_duration.as_secs_f64()
        },
        if moca_duration > rust_duration {
            "slower"
        } else {
            "faster"
        }
    );

    // Verify output matches
    assert_eq!(
        moca_output, rust_output,
        "Output mismatch between moca and Rust implementations!\n\n--- Moca output ---\n{}\n\n--- Rust output ---\n{}",
        moca_output, rust_output
    );

    println!("✓ Outputs match!");
}

#[test]
fn mandelbrot_comparison_50() {
    compare_mandelbrot(50);
}

#[test]
fn mandelbrot_comparison_200() {
    compare_mandelbrot(200);
}

#[test]
fn mandelbrot_comparison_500() {
    compare_mandelbrot(500);
}

/// Verify that the examples/mandelbrot.mc file produces the expected output.
#[test]
fn mandelbrot_example_file() {
    let example_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mandelbrot.mc");

    if !example_path.exists() {
        panic!("examples/mandelbrot.mc does not exist");
    }

    let config = RuntimeConfig::default();
    let (output, result) = run_file_capturing_output(&example_path, &config);

    match result {
        Ok(()) => {
            // Verify output is valid (24 lines, 80 chars each + newline)
            let lines: Vec<&str> = output.stdout.lines().collect();
            assert_eq!(lines.len(), 24, "Expected 24 lines of output");
            for (i, line) in lines.iter().enumerate() {
                assert_eq!(
                    line.len(),
                    80,
                    "Line {} has {} chars, expected 80",
                    i + 1,
                    line.len()
                );
            }

            // Compare with Rust implementation
            let rust_output = mandelbrot_rust(100);
            assert_eq!(
                output.stdout, rust_output,
                "examples/mandelbrot.mc output does not match Rust implementation"
            );

            println!("✓ examples/mandelbrot.mc produces correct output!");
        }
        Err(e) => panic!("examples/mandelbrot.mc execution failed: {}", e),
    }
}
