// Benchmark: Mandelbrot set computation with max_iter=30000
// Simplified version for benchmarking (no output, just computation)
fun mandelbrot_bench(max_iter: int) -> int {
    let width = 80;
    let height = 24;
    let escape_count = 0;

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
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;

            while iter < max_iter {
                let zr2 = zr * zr;
                let zi2 = zi * zi;

                if zr2 + zi2 > 4.0 {
                    escape_count = escape_count + 1;
                    iter = max_iter;
                } else {
                    let new_zr = zr2 - zi2 + cx;
                    let new_zi = 2.0 * zr * zi + cy;
                    zr = new_zr;
                    zi = new_zi;
                    iter = iter + 1;
                }
            }

            cx = cx + x_step;
            px = px + 1;
        }

        cy = cy + y_step;
        py = py + 1;
    }

    return escape_count;
}

print($"{mandelbrot_bench(30000)}");
