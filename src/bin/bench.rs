use fluxdb::engine::runtime::EngineRuntime;
use std::time::Instant;
use clap::Parser;
use serde_json::json;
use futures::future::join_all;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
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
    
    let runtime = EngineRuntime::start();
    let handler = runtime.handle;

    if args.mixed {
        run_mixed_benchmark(handler, args.writes, args.concurrency).await?;
        return Ok(());
    }

    if args.writes > 0 {
        bench_op("SET", handler.clone(), args.writes, args.concurrency, |h, i| {
            let key = format!("key_{}", i);
            let value = json!({"id": i, "data": "benchmark data"});
            async move { h.set(key, value).await }
        }).await?;
    }

    if args.reads > 0 {
        let write_count = args.writes.max(1);
        bench_op("GET", handler.clone(), args.reads, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            async move { h.get(key).await.map(|_| ()) }
        }).await?;
    }

    if args.patches > 0 {
        let write_count = args.writes.max(1);
        bench_op("PATCH", handler.clone(), args.patches, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            let delta = json!({"patched": true, "iter": i});
            async move { h.patch(key, delta).await }
        }).await?;
    }

    if args.deletes > 0 {
        let write_count = args.writes.max(1);
        bench_op("DELETE", handler.clone(), args.deletes, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            async move { h.delete(key).await }
        }).await?;
    }

    Ok(())
}

async fn bench_op<F, Fut>(
    name: &str,
    handler: fluxdb::engine::handler::EngineHandle,
    total_ops: usize,
    concurrency: usize,
    op_fn: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(fluxdb::engine::handler::EngineHandle, usize) -> Fut + Send + Sync + Copy + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send,
{
    println!("Benchmarking {} {} operations (concurrency: {})...", total_ops, name, concurrency);
    let start = Instant::now();
    
    let batch_size = total_ops / concurrency;
    let mut tasks = Vec::new();

    let name_owned = name.to_string();

    for c in 0..concurrency {
        let h = handler.clone();
        let name_task = name_owned.clone();
        let start_idx = c * batch_size;
        let end_idx = if c == concurrency - 1 { total_ops } else { (c + 1) * batch_size };
        
        tasks.push(tokio::spawn(async move {
            let mut durations = Vec::new();
            for i in start_idx..end_idx {
                let op_start = Instant::now();
                if let Err(e) = op_fn(h.clone(), i).await {
                    eprintln!("{} failed at index {}: {}", name_task, i, e);
                    return Err(e);
                }
                durations.push(op_start.elapsed());
            }
            Ok::<Vec<std::time::Duration>, String>(durations)
        }));
    }

    let mut all_durations = Vec::new();
    for res in join_all(tasks).await {
        let durations = res??;
        all_durations.extend(durations);
    }
    
    let duration = start.elapsed();
    print_stats(name, total_ops, duration, &mut all_durations);
    Ok(())
}

async fn run_mixed_benchmark(
    handler: fluxdb::engine::handler::EngineHandle,
    total_ops: usize,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Benchmarking Mixed Workload (total: {} ops, concurrency: {})...", total_ops, concurrency);
    let start = Instant::now();
    
    let batch_size = total_ops / concurrency;
    let mut tasks = Vec::new();

    for c in 0..concurrency {
        let h = handler.clone();
        let start_idx = c * batch_size;
        let end_idx = if c == concurrency - 1 { total_ops } else { (c + 1) * batch_size };
        
        tasks.push(tokio::spawn(async move {
            let mut durations = Vec::new();
            for i in start_idx..end_idx {
                let op_start = Instant::now();
                let key = format!("key_{}", i % 100); 
                match i % 4 {
                    0 => { h.set(key, json!({"v": i})).await.map_err(|e| e.to_string())?; }
                    1 => { h.get(key).await.map_err(|e| e.to_string())?; }
                    2 => { h.patch(key, json!({"p": i})).await.map_err(|e| e.to_string())?; }
                    _ => { h.delete(key).await.map_err(|e| e.to_string())?; }
                }
                durations.push(op_start.elapsed());
            }
            Ok::<Vec<std::time::Duration>, String>(durations)
        }));
    }

    let mut all_durations = Vec::new();
    for res in join_all(tasks).await {
        let durations = res??;
        all_durations.extend(durations);
    }
    
    let duration = start.elapsed();
    print_stats("Mixed Workload", total_ops, duration, &mut all_durations);
    Ok(())
}

fn print_stats(name: &str, total_ops: usize, total_duration: std::time::Duration, latencies: &mut [std::time::Duration]) {
    latencies.sort();
    let p50 = latencies[total_ops / 2];
    let p95 = latencies[(total_ops as f64 * 0.95) as usize];
    let p99 = latencies[(total_ops as f64 * 0.99) as usize];

    let ops_per_sec = total_ops as f64 / total_duration.as_secs_f64();
    println!("------------------------------------");
    println!("{} Performance:", name);
    println!("  Total operations: {}", total_ops);
    println!("  Total time:       {:.2?}", total_duration);
    println!("  Throughput:       {:.2} ops/sec", ops_per_sec);
    println!("  Latency P50:      {:.2?}", p50);
    println!("  Latency P95:      {:.2?}", p95);
    println!("  Latency P99:      {:.2?}", p99);
    println!("  Avg latency:      {:.2?} per op", total_duration / total_ops as u32);
    println!("------------------------------------");
}
