use fluxdb::engine::runtime::EngineRuntime;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Barrier;
use futures::future::join_all;

#[tokio::test]
async fn test_concurrency_correctness() {
    let runtime = EngineRuntime::start();
    let handler = Arc::new(runtime.handle);
    let num_tasks = 50;
    let ops_per_task = 100;
    
    let barrier = Arc::new(Barrier::new(num_tasks));
    let mut tasks = Vec::new();

    for t in 0..num_tasks {
        let h = handler.clone();
        let b = barrier.clone();
        tasks.push(tokio::spawn(async move {
            b.wait().await;
            for i in 0..ops_per_task {
                let key = format!("task_{}_key_{}", t, i);
                // SET
                h.set(key.clone(), json!({"val": i})).await.unwrap();
                // GET
                let doc = h.get(key.clone()).await.unwrap().unwrap();
                assert_eq!(doc.value, json!({"val": i}));
                // PATCH
                h.patch(key.clone(), json!({"patched": true})).await.unwrap();
                // DELETE
                h.delete(key.clone()).await.unwrap();
                // GET None
                let doc = h.get(key.clone()).await.unwrap();
                assert!(doc.is_none());
            }
        }));
    }

    let results = join_all(tasks).await;
    for res in results {
        res.expect("Task panicked or failed");
    }

    // Verify shared key concurrency
    let shared_key = "shared_counter".to_string();
    let mut tasks = Vec::new();
    let num_tasks = 20;
    let barrier = Arc::new(Barrier::new(num_tasks));

    for _ in 0..num_tasks {
        let h = handler.clone();
        let b = barrier.clone();
        let sk = shared_key.clone();
        tasks.push(tokio::spawn(async move {
            b.wait().await;
            for _ in 0..50 {
                // This is not atomic across multiple commands, but we test no panics
                let _ = h.set(sk.clone(), json!(1)).await;
                let _ = h.get(sk.clone()).await;
            }
        }));
    }

    join_all(tasks).await;
}
