use moca::compiler::run_file_capturing_output;
use moca::config::{JitMode, RuntimeConfig};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

#[derive(Serialize)]
struct BenchmarkResult {
    name: String,
    moca_jit_on_secs: f64,
    moca_jit_off_secs: f64,
    rust_time_secs: f64,
}

#[derive(Serialize)]
struct BenchmarkOutput {
    results: Vec<BenchmarkResult>,
}

// Rust reference implementations

fn rust_sum_loop() {
    let mut sum: i64 = 0;
    for i in 1..=1_000_000 {
        sum += i;
    }
    eprintln!("{}", sum);
}

fn rust_nested_loop() {
    let mut sum: i64 = 0;
    for i in 0..500 {
        for j in 0..500 {
            sum += i * j;
        }
    }
    eprintln!("{}", sum);
}

fn rust_fibonacci(n: i32) -> i32 {
    if n <= 1 {
        n
    } else {
        rust_fibonacci(n - 1) + rust_fibonacci(n - 2)
    }
}

fn rust_mandelbrot(max_iter: i32) -> i32 {
    let width = 80;
    let height = 24;
    let mut escape_count = 0;

    let x_min = -2.0_f64;
    let x_max = 1.0_f64;
    let y_min = -1.0_f64;
    let y_max = 1.0_f64;

    let x_step = (x_max - x_min) / 80.0;
    let y_step = (y_max - y_min) / 24.0;

    let mut cy = y_min;
    for _ in 0..height {
        let mut cx = x_min;
        for _ in 0..width {
            let mut zr = 0.0_f64;
            let mut zi = 0.0_f64;
            let mut iter = 0;

            while iter < max_iter {
                let zr2 = zr * zr;
                let zi2 = zi * zi;

                if zr2 + zi2 > 4.0 {
                    escape_count += 1;
                    break;
                }

                let new_zr = zr2 - zi2 + cx;
                let new_zi = 2.0 * zr * zi + cy;
                zr = new_zr;
                zi = new_zi;
                iter += 1;
            }

            cx += x_step;
        }
        cy += y_step;
    }

    escape_count
}

fn time_rust<F>(f: F) -> f64
where
    F: FnOnce(),
{
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64()
}

fn run_moca_benchmark(bench_file: &str, jit_enabled: bool) -> f64 {
    let bench_path = format!(
        "{}/bench/moca/{}.mc",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("/bench"),
        bench_file
    );

    let config = RuntimeConfig {
        jit_mode: if jit_enabled {
            JitMode::On
        } else {
            JitMode::Off
        },
        jit_threshold: 1, // Compile immediately for benchmarking
        ..Default::default()
    };

    let start = Instant::now();
    let (_output, result) = run_file_capturing_output(Path::new(&bench_path), &config);
    let elapsed = start.elapsed().as_secs_f64();

    if let Err(e) = result {
        eprintln!(
            "Moca benchmark {} (jit={}) failed: {}",
            bench_file, jit_enabled, e
        );
    }

    elapsed
}

fn main() {
    let mut results = Vec::new();

    // sum_loop benchmark
    let rust_time = time_rust(rust_sum_loop);
    let moca_jit_on = run_moca_benchmark("sum_loop", true);
    let moca_jit_off = run_moca_benchmark("sum_loop", false);
    results.push(BenchmarkResult {
        name: "sum_loop".to_string(),
        moca_jit_on_secs: moca_jit_on,
        moca_jit_off_secs: moca_jit_off,
        rust_time_secs: rust_time,
    });

    // nested_loop benchmark
    let rust_time = time_rust(rust_nested_loop);
    let moca_jit_on = run_moca_benchmark("nested_loop", true);
    let moca_jit_off = run_moca_benchmark("nested_loop", false);
    results.push(BenchmarkResult {
        name: "nested_loop".to_string(),
        moca_jit_on_secs: moca_jit_on,
        moca_jit_off_secs: moca_jit_off,
        rust_time_secs: rust_time,
    });

    // fibonacci benchmark
    let rust_time = time_rust(|| eprintln!("{}", rust_fibonacci(30)));
    let moca_jit_on = run_moca_benchmark("fibonacci", true);
    let moca_jit_off = run_moca_benchmark("fibonacci", false);
    results.push(BenchmarkResult {
        name: "fibonacci".to_string(),
        moca_jit_on_secs: moca_jit_on,
        moca_jit_off_secs: moca_jit_off,
        rust_time_secs: rust_time,
    });

    // mandelbrot benchmark
    let rust_time = time_rust(|| eprintln!("{}", rust_mandelbrot(200)));
    let moca_jit_on = run_moca_benchmark("mandelbrot", true);
    let moca_jit_off = run_moca_benchmark("mandelbrot", false);
    results.push(BenchmarkResult {
        name: "mandelbrot".to_string(),
        moca_jit_on_secs: moca_jit_on,
        moca_jit_off_secs: moca_jit_off,
        rust_time_secs: rust_time,
    });

    let output = BenchmarkOutput { results };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
