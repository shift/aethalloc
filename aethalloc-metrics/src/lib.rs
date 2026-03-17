//! Prometheus metrics exporter for AethAlloc
//!
//! Usage:
//!   LD_PRELOAD="libaethalloc.so libaethalloc_metrics.so" ./your-program
//!   curl http://localhost:9091/metrics
//!
//! Environment variables:
//!   AETHALLOC_METRICS_PORT - HTTP port (default: 9091)

use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::thread;
use std::time::Duration;

const DEFAULT_PORT: u16 = 9091;
const PORT_ENV: &str = "AETHALLOC_METRICS_PORT";

#[link(name = "aethalloc")]
extern "C" {
    fn aethalloc_get_metrics() -> MetricsSnapshot;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MetricsSnapshot {
    pub allocs: u64,
    pub frees: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub direct_allocs: u64,
}

fn format_metrics() -> String {
    let snapshot = unsafe { aethalloc_get_metrics() };
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

    output.push_str("# HELP aethalloc_cache_hit_rate Cache hit rate percentage\n");
    output.push_str("# TYPE aethalloc_cache_hit_rate gauge\n");
    output.push_str(&format!("aethalloc_cache_hit_rate {:.2}\n", hit_rate));

    output.push_str("# HELP aethalloc_active_blocks Estimated active blocks\n");
    output.push_str("# TYPE aethalloc_active_blocks gauge\n");
    output.push_str(&format!(
        "aethalloc_active_blocks {}\n",
        snapshot.allocs.saturating_sub(snapshot.frees)
    ));

    output
}

fn handle_client(mut stream: std::net::TcpStream) {
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
        ("200 OK", format_metrics())
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

fn run_server(addr: SocketAddr) {
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[aethalloc-metrics] Failed to bind {}: {}", addr, e);
            return;
        }
    };

    eprintln!("[aethalloc-metrics] Listening on http://{}", addr);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_client(stream),
            Err(e) => eprintln!("[aethalloc-metrics] Accept error: {}", e),
        }
    }
}

/// Start the metrics HTTP server in a background thread.
///
/// This function should be called early in your program's initialization.
/// The server will run on the port specified by AETHALLOC_METRICS_PORT
/// environment variable (default: 9091).
pub fn start_metrics_server() {
    let port = env::var(PORT_ENV)
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        run_server(addr);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_metrics() {
        let metrics = format_metrics();
        assert!(metrics.contains("aethalloc_allocs_total"));
        assert!(metrics.contains("aethalloc_cache_hit_rate"));
        assert!(metrics.contains("# TYPE aethalloc_cache_hits_total counter"));
    }
}
