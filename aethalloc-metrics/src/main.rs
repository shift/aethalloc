//! Prometheus metrics exporter for AethAlloc
//!
//! This is a standalone binary that serves metrics HTTP endpoint.
//!
//! For full functionality, aethalloc needs to be compiled with metrics export enabled.
//! The allocator will write metrics to a shared memory segment that this exporter reads.

use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};

const DEFAULT_PORT: u16 = 9091;

#[derive(Debug, Clone, Copy, Default)]
struct MetricsSnapshot {
    allocs: u64,
    frees: u64,
    cache_hits: u64,
    cache_misses: u64,
    direct_allocs: u64,
}

fn format_prometheus_metrics(snapshot: &MetricsSnapshot) -> String {
    let mut output = String::new();

    output.push_str("# HELP aethalloc_allocs_total Total allocations\n");
    output.push_str("# TYPE aethalloc_allocs_total counter\n");
    output.push_str(&format!("aethalloc_allocs_total {}\n", snapshot.allocs));

    output.push_str("# HELP aethalloc_frees_total Total deallocations\n");
    output.push_str("# TYPE aethalloc_frees_total counter\n");
    output.push_str(&format!("aethalloc_frees_total {}\n", snapshot.frees));

    output.push_str("# HELP aethalloc_cache_hits_total Cache hits\n");
    output.push_str("# TYPE aethalloc_cache_hits_total counter\n");
    output.push_str(&format!(
        "aethalloc_cache_hits_total {}\n",
        snapshot.cache_hits
    ));

    output.push_str("# HELP aethalloc_cache_misses_total Cache misses\n");
    output.push_str("# TYPE aethalloc_cache_misses_total counter\n");
    output.push_str(&format!(
        "aethalloc_cache_misses_total {}\n",
        snapshot.cache_misses
    ));

    output.push_str("# HELP aethalloc_direct_allocs_total Direct (large) allocations\n");
    output.push_str("# TYPE aethalloc_direct_allocs_total counter\n");
    output.push_str(&format!(
        "aethalloc_direct_allocs_total {}\n",
        snapshot.direct_allocs
    ));

    let hit_rate = if snapshot.cache_hits + snapshot.cache_misses > 0 {
        snapshot.cache_hits as f64 / (snapshot.cache_hits + snapshot.cache_misses) as f64 * 100.0
    } else {
        0.0
    };

    output.push_str("# HELP aethalloc_cache_hit_rate_percent Cache hit rate percentage\n");
    output.push_str("# TYPE aethalloc_cache_hit_rate_percent gauge\n");
    output.push_str(&format!(
        "aethalloc_cache_hit_rate_percent {:.2}\n",
        hit_rate
    ));

    output.push_str("# HELP aethalloc_active_blocks Estimated active blocks\n");
    output.push_str("# TYPE aethalloc_active_blocks gauge\n");
    output.push_str(&format!(
        "aethalloc_active_blocks {}\n",
        snapshot.allocs.saturating_sub(snapshot.frees)
    ));

    output
}

fn handle_client(mut stream: std::net::TcpStream, metrics: &MetricsSnapshot) {
    let mut buffer = [0u8; 4096];
    let n = match stream.read(&mut buffer) {
        Ok(n) => n,
        Err(_) => return,
    };

    let request = std::str::from_utf8(&buffer[..n]).unwrap_or("");
    let path = request
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");

    let (status, body) = if path == "/metrics" {
        ("200 OK", format_prometheus_metrics(metrics))
    } else if path == "/health" || path == "/" {
        ("200 OK", "OK\n".to_string())
    } else {
        ("404 Not Found", "Not Found\n".to_string())
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
        status,
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn run_server(port: u16, metrics: &MetricsSnapshot) {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[aethalloc-metrics] Failed to bind port {}: {}", port, e);
            return;
        }
    };

    println!("[aethalloc-metrics] Listening on http://0.0.0.0:{}", port);
    println!("[aethalloc-metrics] Metrics available at /metrics");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_client(stream, metrics),
            Err(_) => continue,
        }
    }
}

fn print_usage() {
    println!("AethAlloc Prometheus Metrics Exporter");
    println!();
    println!("Usage: aethalloc-metrics [OPTIONS]");
    println!();
    println!("Options:");
    println!(
        "  -p, --port PORT    HTTP server port (default: {})",
        DEFAULT_PORT
    );
    println!("  -h, --help         Show this help message");
    println!();
    println!("Environment Variables:");
    println!(
        "  AETHALLOC_METRICS_PORT  HTTP server port (default: {})",
        DEFAULT_PORT
    );
    println!();
    println!("Note: This is a placeholder exporter. For full functionality,");
    println!("compile aethalloc with the 'metrics' feature enabled.");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut port = env::var("AETHALLOC_METRICS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(DEFAULT_PORT);
                    i += 2;
                } else {
                    eprintln!("Error: --port requires an argument");
                    std::process::exit(1);
                }
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            arg => {
                eprintln!("Error: Unknown argument: {}", arg);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    println!(
        "[aethalloc-metrics] Starting Prometheus metrics exporter on port {}",
        port
    );
    println!(
        "[aethalloc-metrics] Access metrics at http://localhost:{}/metrics",
        port
    );

    // Placeholder metrics - in full implementation these would be read from
    // shared memory or a file that aethalloc writes to
    let metrics = MetricsSnapshot::default();

    run_server(port, &metrics);
}
