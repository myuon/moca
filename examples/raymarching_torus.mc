// Raymarching ASCII renderer - Torus
// Renders a torus using signed distance functions and Lambert shading
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

    // Torus parameters: major radius R, minor radius r
    let big_r = 0.8;
    let small_r = 0.3;

    let cam_z = 3.0;
    let focal = 1.5;

    // Light direction: normalize(1, 1, 1)
    let lx = 0.5773;
    let ly = 0.5773;
    let lz = 0.5773;

    let half_w = 40.0;
    let half_h = 12.0;
    let eps = 0.001;

    // Rotation: Rx(-30°) to tilt torus toward viewer so hole is visible
    // cos(-30°)=0.866, sin(-30°)=-0.5
    let cos_x = 0.866;
    let sin_x = 0.0 - 0.5;

    // Pre-compute rotated camera origin: Rx * (0, 0, cam_z)
    let cam_ox = 0.0;
    let cam_oy = (0.0 - cam_z) * sin_x;
    let cam_oz = cam_z * cos_x;

    // Pre-compute rotated light direction
    let light_x = lx;
    let light_y = ly * cos_x - lz * sin_x;
    let light_z = ly * sin_x + lz * cos_x;

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

            // Rotate ray direction: Rx(-30°)
            let rdx = dx;
            let rdy = dy * cos_x - dz * sin_x;
            let rdz = dy * sin_x + dz * cos_x;

            // Raymarching with inlined torus SDF
            // Torus SDF: length(vec2(length(p.xz) - R, p.y)) - r
            var t = 0.0;
            var hit_x = 0.0;
            var hit_y = 0.0;
            var hit_z = 0.0;
            var hit = false;
            var step = 0;
            while step < 80 {
                let rx = cam_ox + rdx * t;
                let ry = cam_oy + rdy * t;
                let rz = cam_oz + rdz * t;

                // Inline torus SDF
                let q = sqrt_f(rx * rx + rz * rz) - big_r;
                let d = sqrt_f(q * q + ry * ry) - small_r;

                if d < 0.001 {
                    hit_x = rx;
                    hit_y = ry;
                    hit_z = rz;
                    hit = true;
                    step = 80;
                }
                if !hit {
                    t = t + d;
                    if t > 10.0 {
                        step = 80;
                    }
                }
                step = step + 1;
            }

            if hit {
                // Normal via central differences (inlined torus SDF)
                let qxp = sqrt_f((hit_x + eps) * (hit_x + eps) + hit_z * hit_z) - big_r;
                let qxm = sqrt_f((hit_x - eps) * (hit_x - eps) + hit_z * hit_z) - big_r;
                let qyp = sqrt_f(hit_x * hit_x + hit_z * hit_z) - big_r;
                let qzp = sqrt_f(hit_x * hit_x + (hit_z + eps) * (hit_z + eps)) - big_r;
                let qzm = sqrt_f(hit_x * hit_x + (hit_z - eps) * (hit_z - eps)) - big_r;

                let nx = sqrt_f(qxp * qxp + hit_y * hit_y) - sqrt_f(qxm * qxm + hit_y * hit_y);
                let ny = sqrt_f(qyp * qyp + (hit_y + eps) * (hit_y + eps))
                       - sqrt_f(qyp * qyp + (hit_y - eps) * (hit_y - eps));
                let nz = sqrt_f(qzp * qzp + hit_y * hit_y) - sqrt_f(qzm * qzm + hit_y * hit_y);
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
