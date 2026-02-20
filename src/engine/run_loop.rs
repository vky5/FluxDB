
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use crate::engine::db::Database;
use crate::engine::pending::PendingWrite;
use crate::interface::Command;

/// Runs the single-writer database actor loop.
///
/// Owns:
/// - WAL fsync batching
/// - pending write queue
/// - post-durability apply + notify + ACK
///
/// `main` must NOT contain any of this logic.
pub async fn run_single_writer_loop(mut rx: mpsc::Receiver<Command>) {
    // open DB inside the writer
    let mut db = Database::open("./fluxdb").expect("failed to open database");

    // fsync batching timer
    let mut tick = interval(Duration::from_millis(5));

    // pending writes waiting for durability barrier
    let mut pending: Vec<PendingWrite> = Vec::new();

    // serialized execution loop (database actor)
    loop {
        tokio::select! {
            // -------- receive command --------
            Some(cmd) = rx.recv() => {
                match cmd {
                    Command::Set { key, value, resp } => match db.put(key, value) {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },

                    Command::Get { key, resp } => {
                        let result = db.get(&key).cloned();
                        let _ = resp.send(result);
                    }

                    Command::Del { key, resp } => match db.delete(&key) {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },

                    Command::Patch { key, delta, resp } => match db.patch(&key, delta) {
                        Ok(event) => pending.push(PendingWrite { event, resp }),
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
                    },

                    Command::Snapshot {resp} => match db.checkpoint("flux.wal"){
                        Ok(_) => { let _ = resp.send(Ok(())); }
                        Err(e) => { let _ = resp.send(Err(e.to_string())); }
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
                    if let Err(e) = db.execute_post_durability(p.event) {
                        let _ = p.resp.send(Err(e.to_string()));
                    } else {
                        let _ = p.resp.send(Ok(()));
                    }
                }
            }
        }
    }
}
