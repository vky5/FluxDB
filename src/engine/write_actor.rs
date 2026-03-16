use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, interval};

use crate::engine::db::Database;
use crate::engine::notify_actor::NotifyCommand;
use crate::engine::pending::PendingWrite;
use crate::engine::snapshot_actor::SnapshotActorCommand;
use crate::interface::command::WriteCommand;
use crate::store::kv::Store;

/// Runs the single-writer database actor loop.
///
/// Owns:
/// - WAL fsync batching
/// - pending write queue
/// - post-durability apply + notify + ACK
///
/// `main` must NOT contain any of this logic.
/// Single write loop
pub async fn write_actor(
    mut rx: mpsc::Receiver<WriteCommand>,
    shared_store: Arc<RwLock<Store>>,
    snap_tx: mpsc::Sender<SnapshotActorCommand>,
    notify_tx: mpsc::Sender<NotifyCommand>,
) {
    // open DB inside the writer
    let mut db = Database::open("./fluxdb", shared_store)
        .await
        .expect("failed to open database");

    // fsync batching timer
    let mut tick = interval(Duration::from_millis(5));

    // pending writes waiting for durability barrier
    let mut pending: Vec<PendingWrite> = Vec::new();

    let mut writes_since_snapshot: u64 = 0;
    const SNAPSHOT_EVERY: u64 = 1000;

    // serialized execution loop (database actor)
    loop {

        // the drain phase 9o
        // 1. Wait for at least one command OR a tick
        tokio::select! {
            res = rx.recv() => { // recv blocks until a message is received
                match res {
                    Some(cmd) => handle_write_command(&mut db, &mut pending, cmd).await,
                    None => break, // Channel closed, exit actor
                }
            }
            _ = tick.tick() => { // a safety mechanism, if there are no writes for 5ms, we will fsync anyway (in future if we implmenet length based fsync batching)
                // tick is just a heartbeat for idle writes
            }
        }

        // 2. Opportunistically drain all currently available commands
        while let Ok(cmd) = rx.try_recv() { // try_recv is non-blocking and drains all messages quickly (using this directly and only this will consume 100 percent CPU)
            handle_write_command(&mut db, &mut pending, cmd).await;
        }

        // 3. If we have pending writes, fsync immediately
        if !pending.is_empty() {
            // durability barrier
            if let Err(e) = db.fsync_wal() {
                for p in pending.drain(..) {
                    let _ = p.resp.send(Err(e.to_string()));
                }
                continue;
            }

            // apply + notify + ACK
            for p in pending.drain(..) {
                let event = p.event.clone();
                if let Err(e) = db.execute_post_durability(p.event).await {
                    let _ = p.resp.send(Err(e.to_string()));
                } else {
                    let _ = p.resp.send(Ok(()));
                    let _ = notify_tx.send(NotifyCommand::Dispatch { event }).await;
                    writes_since_snapshot += 1;
                    if writes_since_snapshot >= SNAPSHOT_EVERY {
                        let _ = snap_tx.send(SnapshotActorCommand::TriggerNow).await;
                        writes_since_snapshot = 0;
                    }
                }
            }
        }
    }
}

async fn handle_write_command(db: &mut Database, pending: &mut Vec<PendingWrite>, cmd: WriteCommand) {
    match cmd {
        WriteCommand::Set { key, value, resp } => match db.put(key, value).await {
            Ok(event) => pending.push(PendingWrite { event, resp }),
            Err(e) => {
                let _ = resp.send(Err(e.to_string()));
            }
        },
        WriteCommand::Del { key, resp } => match db.delete(&key).await {
            Ok(event) => pending.push(PendingWrite { event, resp }),
            Err(e) => {
                let _ = resp.send(Err(e.to_string()));
            }
        },
        WriteCommand::Patch { key, delta, resp } => match db.patch(&key, delta).await {
            Ok(event) => pending.push(PendingWrite { event, resp }),
            Err(e) => {
                let _ = resp.send(Err(e.to_string()));
            }
        },
        WriteCommand::Snapshot { resp } => match db.checkpoint_payload().await {
            Ok(snapshot) => {
                let _ = resp.send(Ok(snapshot));
            }
            Err(e) => {
                let _ = resp.send(Err(e.to_string()));
            }
        },
        WriteCommand::InjectFailure { resp } => {
            db.fail_next_fsync = true;
            let _ = resp.send(());
        }
    }
}
