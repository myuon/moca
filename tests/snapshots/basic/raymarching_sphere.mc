// Raymarching ASCII renderer - Sphere
// Renders a sphere using signed distance functions and Lambert shading
// Output: 80x24 ASCII art

// ASCII gradient from dark to bright (10 levels)
fun shade_char(brightness: float) {
    if brightness <= 0.0 { print_str(" "); return; }
    if brightness < 0.1 { print_str("."); return; }
    if brightness < 0.2 { print_str(":"); return; }
    if brightness < 0.3 { print_str("-"); return; }
    if brightness < 0.4 { print_str("="); return; }
    if brightness < 0.5 { print_str("+"); return; }
    if brightness < 0.6 { print_str("*"); return; }
    if brightness < 0.7 { print_str("#"); return; }
    if brightness < 0.8 { print_str("%"); return; }
    print_str("@");
}

fun render() {
    let width = 80;
    let height = 24;
    let radius = 1.0;

    // Camera at (0, 0, 3), looking toward origin
    let cam_z = 3.0;
    let focal = 1.5;

    // Light direction (toward light source): normalize(1, 1, 1)
    let lx = 0.5773;
    let ly = 0.5773;
    let lz = 0.5773;

    let half_w = 40.0;
    let half_h = 12.0;
    let eps = 0.001;

    let py = 0;
    while py < height {
        let px = 0;
        while px < width {
            // Map pixel to ray direction
            let u = (_int_to_float(px) - half_w + 0.5) / half_w;
            let v = (half_h - _int_to_float(py) - 0.5) / half_w * 2.0;

            // Ray direction (perspective projection)
            let rd_len = sqrt_f(u * u + v * v + focal * focal);
            let dx = u / rd_len;
            let dy = v / rd_len;
            let dz = (0.0 - focal) / rd_len;

            // Raymarching loop (inline SDF: sqrt(x^2+y^2+z^2) - radius)
            let t = 0.0;
            let hit_x = 0.0;
            let hit_y = 0.0;
            let hit_z = 0.0;
            let hit = false;
            let step = 0;
            while step < 64 {
                let rx = dx * t;
                let ry = dy * t;
                let rz = cam_z + dz * t;
                let d = sqrt_f(rx * rx + ry * ry + rz * rz) - radius;
                if d < 0.001 {
                    hit_x = rx;
                    hit_y = ry;
                    hit_z = rz;
                    hit = true;
                    step = 64;
                }
                if !hit {
                    t = t + d;
                    if t > 10.0 {
                        step = 64;
                    }
                }
                step = step + 1;
            }

            if hit {
                // Compute normal via central differences (inlined SDF)
                let nx = sqrt_f((hit_x + eps) * (hit_x + eps) + hit_y * hit_y + hit_z * hit_z)
                       - sqrt_f((hit_x - eps) * (hit_x - eps) + hit_y * hit_y + hit_z * hit_z);
                let ny = sqrt_f(hit_x * hit_x + (hit_y + eps) * (hit_y + eps) + hit_z * hit_z)
                       - sqrt_f(hit_x * hit_x + (hit_y - eps) * (hit_y - eps) + hit_z * hit_z);
                let nz = sqrt_f(hit_x * hit_x + hit_y * hit_y + (hit_z + eps) * (hit_z + eps))
                       - sqrt_f(hit_x * hit_x + hit_y * hit_y + (hit_z - eps) * (hit_z - eps));
                let n_len = sqrt_f(nx * nx + ny * ny + nz * nz);

                // Lambert shading
                let dot_val = (nx / n_len) * lx + (ny / n_len) * ly + (nz / n_len) * lz;
                let brightness = dot_val;
                if brightness < 0.0 {
                    brightness = 0.0;
                }
                shade_char(brightness);
            } else {
                print_str(" ");
            }
            px = px + 1;
        }
        print_str("\n");
        py = py + 1;
    }
}

render();
