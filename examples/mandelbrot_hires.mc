// Mandelbrot set - High resolution ASCII art with 70-level gradient
// Uses character density gradient for smooth shading
// Resolution: 120x40, 200 iterations

// 70-level density gradient (dark to bright)
// " .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"
fun print_char(idx: int) {
    if idx == 0 { print_str(" "); return; }
    if idx == 1 { print_str("."); return; }
    if idx == 2 { print_str("'"); return; }
    if idx == 3 { print_str("`"); return; }
    if idx == 4 { print_str("^"); return; }
    if idx == 5 { print_str(","); return; }
    if idx == 6 { print_str(":"); return; }
    if idx == 7 { print_str(";"); return; }
    if idx == 8 { print_str("I"); return; }
    if idx == 9 { print_str("l"); return; }
    if idx == 10 { print_str("!"); return; }
    if idx == 11 { print_str("i"); return; }
    if idx == 12 { print_str(">"); return; }
    if idx == 13 { print_str("<"); return; }
    if idx == 14 { print_str("~"); return; }
    if idx == 15 { print_str("+"); return; }
    if idx == 16 { print_str("_"); return; }
    if idx == 17 { print_str("-"); return; }
    if idx == 18 { print_str("?"); return; }
    if idx == 19 { print_str("]"); return; }
    if idx == 20 { print_str("["); return; }
    if idx == 21 { print_str("}"); return; }
    if idx == 22 { print_str("{"); return; }
    if idx == 23 { print_str("1"); return; }
    if idx == 24 { print_str(")"); return; }
    if idx == 25 { print_str("("); return; }
    if idx == 26 { print_str("|"); return; }
    if idx == 27 { print_str("t"); return; }
    if idx == 28 { print_str("f"); return; }
    if idx == 29 { print_str("j"); return; }
    if idx == 30 { print_str("r"); return; }
    if idx == 31 { print_str("x"); return; }
    if idx == 32 { print_str("n"); return; }
    if idx == 33 { print_str("u"); return; }
    if idx == 34 { print_str("v"); return; }
    if idx == 35 { print_str("c"); return; }
    if idx == 36 { print_str("z"); return; }
    if idx == 37 { print_str("X"); return; }
    if idx == 38 { print_str("Y"); return; }
    if idx == 39 { print_str("U"); return; }
    if idx == 40 { print_str("J"); return; }
    if idx == 41 { print_str("C"); return; }
    if idx == 42 { print_str("L"); return; }
    if idx == 43 { print_str("Q"); return; }
    if idx == 44 { print_str("0"); return; }
    if idx == 45 { print_str("O"); return; }
    if idx == 46 { print_str("Z"); return; }
    if idx == 47 { print_str("m"); return; }
    if idx == 48 { print_str("w"); return; }
    if idx == 49 { print_str("q"); return; }
    if idx == 50 { print_str("p"); return; }
    if idx == 51 { print_str("d"); return; }
    if idx == 52 { print_str("b"); return; }
    if idx == 53 { print_str("k"); return; }
    if idx == 54 { print_str("h"); return; }
    if idx == 55 { print_str("a"); return; }
    if idx == 56 { print_str("o"); return; }
    if idx == 57 { print_str("*"); return; }
    if idx == 58 { print_str("#"); return; }
    if idx == 59 { print_str("M"); return; }
    if idx == 60 { print_str("W"); return; }
    if idx == 61 { print_str("&"); return; }
    if idx == 62 { print_str("8"); return; }
    if idx == 63 { print_str("%"); return; }
    if idx == 64 { print_str("B"); return; }
    if idx == 65 { print_str("@"); return; }
    print_str("$");
}

fun mandelbrot(max_iter: int) {
    let width = 120;
    let height = 40;
    let num_chars = 66;

    let x_min = -2.0;
    let x_max = 0.8;
    let y_min = -1.2;
    let y_max = 1.2;

    let x_step = (x_max - x_min) / 120.0;
    let y_step = (y_max - y_min) / 40.0;

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
                    iter = max_iter + iter + 1;
                }
                if iter < max_iter {
                    let new_zr = zr2 - zi2 + cx;
                    let new_zi = 2.0 * zr * zi + cy;
                    zr = new_zr;
                    zi = new_zi;
                    iter = iter + 1;
                }
            }

            let char_idx = 0;
            if iter > max_iter {
                let escaped_iter = iter - max_iter - 1;
                char_idx = escaped_iter * num_chars / max_iter;
                if char_idx > num_chars {
                    char_idx = num_chars;
                }
            }

            print_char(char_idx);
            cx = cx + x_step;
            px = px + 1;
        }

        print_str("\n");
        cy = cy + y_step;
        py = py + 1;
    }
}

mandelbrot(200);
