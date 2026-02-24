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
async fn main() {
    let args = Args::parse();
    
    let runtime = EngineRuntime::start();
    let handler = runtime.handle;

    if args.mixed {
        run_mixed_benchmark(handler, args.writes, args.concurrency).await;
        return;
    }

    if args.writes > 0 {
        bench_op("SET", handler.clone(), args.writes, args.concurrency, |h, i| {
            let key = format!("key_{}", i);
            let value = json!({"id": i, "data": "benchmark data"});
            async move { h.set(key, value).await }
        }).await;
    }

    if args.reads > 0 {
        let write_count = args.writes.max(1);
        bench_op("GET", handler.clone(), args.reads, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            async move { h.get(key).await.map(|_| ()) }
        }).await;
    }

    if args.patches > 0 {
        let write_count = args.writes.max(1);
        bench_op("PATCH", handler.clone(), args.patches, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            let delta = json!({"patched": true, "iter": i});
            async move { h.patch(key, delta).await }
        }).await;
    }

    if args.deletes > 0 {
        let write_count = args.writes.max(1);
        bench_op("DELETE", handler.clone(), args.deletes, args.concurrency, move |h, i| {
            let key = format!("key_{}", i % write_count);
            async move { h.delete(key).await }
        }).await;
    }
}

async fn bench_op<F, Fut>(
    name: &str,
    handler: fluxdb::engine::handler::EngineHandle,
    total_ops: usize,
    concurrency: usize,
    op_fn: F,
) where
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
            for i in start_idx..end_idx {
                if let Err(e) = op_fn(h.clone(), i).await {
                    eprintln!("{} failed at index {}: {}", name_task, i, e);
                    return Err(e);
                }
            }
            Ok(())
        }));
    }

    join_all(tasks).await;
    
    let duration = start.elapsed();
    let ops_per_sec = total_ops as f64 / duration.as_secs_f64();
    println!("------------------------------------");
    println!("{} Performance:", name);
    println!("  Total operations: {}", total_ops);
    println!("  Total time:       {:.2?}", duration);
    println!("  Throughput:       {:.2} ops/sec", ops_per_sec);
    println!("  Avg latency:      {:.2?} per op", duration / total_ops as u32);
    println!("------------------------------------");
}

async fn run_mixed_benchmark(
    handler: fluxdb::engine::handler::EngineHandle,
    total_ops: usize,
    concurrency: usize,
) {
    println!("Benchmarking Mixed Workload (total: {} ops, concurrency: {})...", total_ops, concurrency);
    let start = Instant::now();
    
    let batch_size = total_ops / concurrency;
    let mut tasks = Vec::new();

    for c in 0..concurrency {
        let h = handler.clone();
        let start_idx = c * batch_size;
        let end_idx = if c == concurrency - 1 { total_ops } else { (c + 1) * batch_size };
        
        tasks.push(tokio::spawn(async move {
            for i in start_idx..end_idx {
                let key = format!("key_{}", i % 100); // reuse keys to stress reactivity/updates
                match i % 4 {
                    0 => { let _ = h.set(key, json!({"v": i})).await; }
                    1 => { let _ = h.get(key).await; }
                    2 => { let _ = h.patch(key, json!({"p": i})).await; }
                    _ => { let _ = h.delete(key).await; }
                }
            }
            Ok::<(), String>(())
        }));
    }

    join_all(tasks).await;
    
    let duration = start.elapsed();
    let ops_per_sec = total_ops as f64 / duration.as_secs_f64();
    println!("------------------------------------");
    println!("Mixed Workload Performance:");
    println!("  Total operations: {}", total_ops);
    println!("  Total time:       {:.2?}", duration);
    println!("  Throughput:       {:.2} ops/sec", ops_per_sec);
    println!("  Avg latency:      {:.2?} per op", duration / total_ops as u32);
    println!("------------------------------------");
}
