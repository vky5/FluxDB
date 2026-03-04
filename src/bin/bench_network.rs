use clap::Parser;
use fluxdb::net::protocol::{Request, Response};
use futures::future::join_all;
use serde_json::json;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Parser, Debug)]
#[command(author, version, about = "FluxDB Network Benchmarker")]
struct Args {
    /// Server address
    #[arg(long, default_value = "127.0.0.1:7000")]
    addr: String,

    /// Number of write (SET) operations
    #[arg(short, long, default_value_t = 1000)]
    writes: usize,

    /// Number of read (GET) operations
    #[arg(short, long, default_value_t = 0)]
    reads: usize,

    /// Number of PATCH operations
    #[arg(short, long, default_value_t = 0)]
    patches: usize,

    /// Number of DELETE operations
    #[arg(short, long, default_value_t = 0)]
    deletes: usize,

    /// Number of concurrent tasks
    #[arg(short, long, default_value_t = 10)]
    concurrency: usize,

    /// Run a mixed workload (writes, reads, patches, deletes)
    #[arg(short, long, default_value_t = false)]
    mixed: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("Starting network benchmark on {}...", args.addr);

    if args.mixed {
        run_mixed_benchmark(&args.addr, args.writes, args.concurrency).await?;
        return Ok(());
    }

    if args.writes > 0 {
        bench_op(
            "SET",
            &args.addr,
            args.writes,
            args.concurrency,
            |i| {
                let key = format!("key_{}", i);
                let value = json!({"id": i, "data": "benchmark data"});
                Request::Set { key, value }
            },
        )
        .await?;
    }

    if args.reads > 0 {
        let write_count = args.writes.max(1);
        bench_op(
            "GET",
            &args.addr,
            args.reads,
            args.concurrency,
            move |i| {
                let key = format!("key_{}", i % write_count);
                Request::Get { key }
            },
        )
        .await?;
    }

    if args.patches > 0 {
        let write_count = args.writes.max(1);
        bench_op(
            "PATCH",
            &args.addr,
            args.patches,
            args.concurrency,
            move |i| {
                let key = format!("key_{}", i % write_count);
                let delta = json!({"patched": true, "iter": i});
                Request::Patch { key, delta }
            },
        )
        .await?;
    }

    if args.deletes > 0 {
        let write_count = args.writes.max(1);
        bench_op(
            "DELETE",
            &args.addr,
            args.deletes,
            args.concurrency,
            move |i| {
                let key = format!("key_{}", i % write_count);
                Request::Del { key }
            },
        )
        .await?;
    }

    Ok(())
}

async fn run_mixed_benchmark(
    addr: &str,
    total_ops: usize,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Benchmarking Mixed Workload (total: {} ops, concurrency: {})...",
        total_ops, concurrency
    );
    let start = Instant::now();

    let ops_per_conn = total_ops / concurrency;
    let mut tasks = Vec::new();

    for c in 0..concurrency {
        let addr = addr.to_string();
        let start_idx = c * ops_per_conn;
        let end_idx = if c == concurrency - 1 {
            total_ops
        } else {
            (c + 1) * ops_per_conn
        };

        tasks.push(tokio::spawn(async move {
            let mut durations = Vec::new();
            let stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
            stream.set_nodelay(true).map_err(|e| e.to_string())?;

            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            for i in start_idx..end_idx {
                let op_start = Instant::now();
                let key = format!("key_{}", i % 100);
                let req = match i % 4 {
                    0 => Request::Set {
                        key,
                        value: json!({"v": i}),
                    },
                    1 => Request::Get { key },
                    2 => Request::Patch {
                        key,
                        delta: json!({"p": i}),
                    },
                    _ => Request::Del { key },
                };

                let mut req_bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
                req_bytes.push(b'\n');

                write_half
                    .write_all(&req_bytes)
                    .await
                    .map_err(|e| e.to_string())?;

                line.clear();
                let n = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("Connection closed by server".to_string());
                }

                let _resp: Response =
                    serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                durations.push(op_start.elapsed());
            }

            Ok::<Vec<std::time::Duration>, String>(durations)
        }));
    }

    let mut all_durations = Vec::new();
    for res in join_all(tasks).await {
        match res {
            Ok(Ok(durations)) => all_durations.extend(durations),
            Ok(Err(e)) => eprintln!("Task failed: {}", e),
            Err(e) => eprintln!("Join error: {}", e),
        }
    }

    if all_durations.is_empty() {
        return Err("No successful operations".into());
    }

    let duration = start.elapsed();
    print_stats(
        "Mixed Workload",
        all_durations.len(),
        duration,
        &mut all_durations,
    );
    Ok(())
}

async fn bench_op<F>(
    name: &str,
    addr: &str,
    total_ops: usize,
    concurrency: usize,
    req_fn: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(usize) -> Request + Send + Sync + Copy + 'static,
{
    println!(
        "Benchmarking {} {} operations (concurrency: {})...",
        total_ops, name, concurrency
    );
    let start = Instant::now();

    let ops_per_conn = total_ops / concurrency;
    let mut tasks = Vec::new();

    for c in 0..concurrency {
        let addr = addr.to_string();
        let start_idx = c * ops_per_conn;
        let end_idx = if c == concurrency - 1 {
            total_ops
        } else {
            (c + 1) * ops_per_conn
        };

        tasks.push(tokio::spawn(async move {
            let mut durations = Vec::new();
            let stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
            stream.set_nodelay(true).map_err(|e| e.to_string())?;
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            for i in start_idx..end_idx {
                let op_start = Instant::now();
                let req = req_fn(i);
                let mut req_bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
                req_bytes.push(b'\n');

                write_half
                    .write_all(&req_bytes)
                    .await
                    .map_err(|e| e.to_string())?;

                line.clear();
                let n = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("Connection closed by server".to_string());
                }

                let _resp: Response =
                    serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                durations.push(op_start.elapsed());
            }

            Ok::<Vec<std::time::Duration>, String>(durations)
        }));
    }

    let mut all_durations = Vec::new();
    for res in join_all(tasks).await {
        match res {
            Ok(Ok(durations)) => all_durations.extend(durations),
            Ok(Err(e)) => eprintln!("Task failed: {}", e),
            Err(e) => eprintln!("Join error: {}", e),
        }
    }

    if all_durations.is_empty() {
        return Err("No successful operations".into());
    }

    let duration = start.elapsed();
    print_stats(name, all_durations.len(), duration, &mut all_durations);
    Ok(())
}

fn print_stats(
    name: &str,
    total_ops: usize,
    total_duration: std::time::Duration,
    latencies: &mut [std::time::Duration],
) {
    latencies.sort();
    let p50 = latencies[total_ops / 2];
    let p95 = latencies[(total_ops as f64 * 0.95) as usize];
    let p99 = latencies[(total_ops as f64 * 0.99) as usize];

    let ops_per_sec = total_ops as f64 / total_duration.as_secs_f64();
    println!("------------------------------------");
    println!("{} Performance (Network):", name);
    println!("  Total operations: {}", total_ops);
    println!("  Total time:       {:.2?}", total_duration);
    println!("  Throughput:       {:.2} ops/sec", ops_per_sec);
    println!("  Latency P50:      {:.2?}", p50);
    println!("  Latency P95:      {:.2?}", p95);
    println!("  Latency P99:      {:.2?}", p99);
    println!("  Avg latency:      {:.2?} per op", total_duration / total_ops as u32);
    println!("------------------------------------");
}
