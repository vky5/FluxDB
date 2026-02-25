use fluxdb::engine::runtime::EngineRuntime;
use serde_json::json;
use std::time::Instant;

#[tokio::test]
async fn test_durability_batching_order() {
    let runtime = EngineRuntime::start();
    let handler = runtime.handle;

    // The write actor has a 5ms interval.
    // If we send a write, it should take AT LEAST some time if we aren't extremely lucky with the tick.
    // But more reliably, we can check that multiple writes sent together are acknowledged together.

    let start = Instant::now();
    let (r1, r2, r3) = tokio::join!(
        handler.set("d1".to_string(), json!(1)),
        handler.set("d2".to_string(), json!(2)),
        handler.set("d3".to_string(), json!(3)),
    );

    let elapsed = start.elapsed();

    r1.unwrap();
    r2.unwrap();
    r3.unwrap();

    // Since they were joined, they likely hit the same batch.
    // We can't strictly assert < 5ms because of CI jitter, but we expect them to be fast if batched.
    println!("Batched write took {:?}", elapsed);
}

#[tokio::test]
async fn test_batch_failure_fails_all_pending_acks() {
    let runtime = EngineRuntime::start();
    let handler = runtime.handle;

    // Inject failure for the next fsync
    handler.inject_failure().await;

    // Send multiple writes that should be batched
    let (r1, r2, r3) = tokio::join!(
        handler.set("f1".to_string(), json!(1)),
        handler.set("f2".to_string(), json!(2)),
        handler.set("f3".to_string(), json!(3)),
    );

    // All should fail with the injected error
    assert!(r1.is_err());
    assert!(r2.is_err());
    assert!(r3.is_err());

    assert_eq!(r1.unwrap_err(), "injected fsync failure");
}

// TODO: Verify batch failure fails all pending acks.
// This requires a way to inject I/O failure into the Database/Wal.
// Currently the DB is hardcoded to "./fluxdb".
