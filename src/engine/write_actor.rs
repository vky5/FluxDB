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
        tokio::select! {
            // -------- receive WriteCommand --------
            Some(cmd) = rx.recv() => {
                match cmd {
                    WriteCommand::Set { key, value, resp } => match db.put(key, value).await {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },
                    WriteCommand::Del { key, resp } => match db.delete(&key).await {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },

                    WriteCommand::Patch { key, delta, resp } => match db.patch(&key, delta).await {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },

                    WriteCommand::Snapshot {resp} => match db.checkpoint_payload().await{
                        Ok(snapshot) => { let _ = resp.send(Ok(snapshot)); }
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },
                    WriteCommand::InjectFailure { resp } => {
                        db.fail_next_fsync = true;
                        let _ = resp.send(());
                    }
                }
            }

            // -------- fsync batch boundary --------
            _ = tick.tick() => {
                if pending.is_empty() {
                    continue;
                }

                // 1. durability barrier
                if let Err(e) = db.fsync_wal() {
                    // fail ALL pending writes
                    for p in pending.drain(..) {
                        let _ = p.resp.send(Err(e.to_string()));
                    }
                    continue;
                }

                // 2. apply + notify + ACK
                for p in pending.drain(..) {

                    let event = p.event.clone();
                    if let Err(e) = db.execute_post_durability(p.event).await {
                        let _ = p.resp.send(Err(e.to_string()));

                    } else {
                        let _ = p.resp.send(Ok(()));
                        let _ = notify_tx.send(NotifyCommand::Dispatch { event }).await;

                        // trigger snapshot automatically after 1000 successful batch fsync
                        writes_since_snapshot+=1;

                        if writes_since_snapshot>=SNAPSHOT_EVERY {
                            let _ = snap_tx.send(SnapshotActorCommand::TriggerNow).await;
                            writes_since_snapshot = 0;
                        }


                    }
                }
            }
        }
    }
}
