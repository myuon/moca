// Mandelbrot set ASCII art generator
// Output: 80x24 ASCII art representation of the Mandelbrot set
//
// Character set: " .:-=+*#%@" (10 characters, index 0-9)
// - Space = inside the Mandelbrot set (never escapes)
// - @ = fastest divergence (escapes immediately)

// Print a single character based on iteration index
fun print_char(idx: int) {
    if idx == 0 { print_str(" "); return; }
    if idx == 1 { print_str("."); return; }
    if idx == 2 { print_str(":"); return; }
    if idx == 3 { print_str("-"); return; }
    if idx == 4 { print_str("="); return; }
    if idx == 5 { print_str("+"); return; }
    if idx == 6 { print_str("*"); return; }
    if idx == 7 { print_str("#"); return; }
    if idx == 8 { print_str("%"); return; }
    print_str("@");
}

// Generate and print Mandelbrot set ASCII art
fun mandelbrot(max_iter: int) {
    let width = 80;
    let height = 24;

    // Coordinate range: real [-2.0, 1.0], imag [-1.0, 1.0]
    let x_min = -2.0;
    let x_max = 1.0;
    let y_min = -1.0;
    let y_max = 1.0;

    let x_step = (x_max - x_min) / 80.0;
    let y_step = (y_max - y_min) / 24.0;

    let cy = y_min;
    let py = 0;
    while py < height {
        let cx = x_min;
        let px = 0;
        while px < width {
            // Mandelbrot iteration: z = z^2 + c
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;

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
                    iter = iter + 1;
                }
            }

            // Select character based on iteration count
            let char_idx = 0;
            if iter > max_iter {
                // Escaped: map iteration count to character index (0-9)
                let escaped_iter = iter - max_iter - 1;
                char_idx = escaped_iter * 9 / max_iter;
                if char_idx > 9 {
                    char_idx = 9;
                }
            }
            // If iter == max_iter, point is in the set (char_idx = 0, space)

            print_char(char_idx);
            cx = cx + x_step;
            px = px + 1;
        }

        print_str("\n");
        cy = cy + y_step;
        py = py + 1;
    }
}

// Main: generate and print Mandelbrot set with default 100 iterations
mandelbrot(100);
