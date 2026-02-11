// Raymarching ASCII renderer - Cube
// Renders a cube using signed distance functions and Lambert shading
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

// max of two floats
fun max_f(a: float, b: float) -> float {
    if a > b { return a; }
    return b;
}

// SDF for axis-aligned box centered at origin with half-size s
fun sdf_box(px: float, py: float, pz: float, s: float) -> float {
    let dx = abs_f(px) - s;
    let dy = abs_f(py) - s;
    let dz = abs_f(pz) - s;
    return max_f(dx, max_f(dy, dz));
}

fun render() {
    let width = 80;
    let height = 24;
    let size = 0.8;
    let cam_z = 3.0;
    let focal = 1.5;

    // Light direction: normalize(1, 1, 2) - illuminates front face too
    let lx = 0.4082;
    let ly = 0.4082;
    let lz = 0.8165;

    let half_w = 40.0;
    let half_h = 12.0;
    let eps = 0.001;

    // Rotation R = Rx(-25°) * Ry(30°) to show front, right, and top faces
    // Ry(30°): cos=0.866, sin=0.5
    // Rx(-25°): cos=0.9063, sin=-0.4226
    let cos_y = 0.866;
    let sin_y = 0.5;
    let cos_x = 0.9063;
    let sin_x = 0.0 - 0.4226;

    // Pre-compute rotated camera origin: R * (0, 0, cam_z)
    // After Ry: (cam_z*sin_y, 0, cam_z*cos_y)
    // After Rx: (same_x, -cam_z*cos_y*sin_x, cam_z*cos_y*cos_x)
    let cam_ox = cam_z * sin_y;
    let cam_oy = (0.0 - cam_z * cos_y) * sin_x;
    let cam_oz = cam_z * cos_y * cos_x;

    // Pre-compute rotated light direction
    let lx1 = lx * cos_y + lz * sin_y;
    let ly1 = ly;
    let lz1 = (0.0 - lx) * sin_y + lz * cos_y;
    let light_x = lx1;
    let light_y = ly1 * cos_x - lz1 * sin_x;
    let light_z = ly1 * sin_x + lz1 * cos_x;

    var py = 0;
    while py < height {
        var px = 0;
        while px < width {
            let u = (_int_to_float(px) - half_w + 0.5) / half_w;
            let v = (half_h - _int_to_float(py) - 0.5) / half_w * 2.0;

            let rd_len = sqrt_f(u * u + v * v + focal * focal);
            let dx = u / rd_len;
            let dy = v / rd_len;
            let dz = (0.0 - focal) / rd_len;

            // Rotate ray direction: R = Rx * Ry
            let dx1 = dx * cos_y + dz * sin_y;
            let dy1 = dy;
            let dz1 = (0.0 - dx) * sin_y + dz * cos_y;
            let rdx = dx1;
            let rdy = dy1 * cos_x - dz1 * sin_x;
            let rdz = dy1 * sin_x + dz1 * cos_x;

            // Raymarching
            var t = 0.0;
            var hit_x = 0.0;
            var hit_y = 0.0;
            var hit_z = 0.0;
            var hit = false;
            var step = 0;
            while step < 64 {
                let rx = cam_ox + rdx * t;
                let ry = cam_oy + rdy * t;
                let rz = cam_oz + rdz * t;
                let d = sdf_box(rx, ry, rz, size);
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
                // Normal via central differences
                let nx = sdf_box(hit_x + eps, hit_y, hit_z, size)
                       - sdf_box(hit_x - eps, hit_y, hit_z, size);
                let ny = sdf_box(hit_x, hit_y + eps, hit_z, size)
                       - sdf_box(hit_x, hit_y - eps, hit_z, size);
                let nz = sdf_box(hit_x, hit_y, hit_z + eps, size)
                       - sdf_box(hit_x, hit_y, hit_z - eps, size);
                let n_len = sqrt_f(nx * nx + ny * ny + nz * nz);

                let dot_val = (nx / n_len) * light_x + (ny / n_len) * light_y + (nz / n_len) * light_z;
                var brightness = dot_val;
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
