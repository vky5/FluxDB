#![allow(dead_code)]

mod command;
mod db;
mod snapshot;
mod store;

use command::Command;
use serde_json::Value;
use std::io::{self, Write};
use tokio::sync::{mpsc, oneshot};

use db::Database;

use crate::store::Event;

// Represents a write that has been executed
// but is waiting for the durability barrier (fsync)
// before the client can be acknowledged.
struct PendingWrite {
    event: Event,
    resp: oneshot::Sender<Result<(), String>>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // create command queue
    let (tx, mut rx) = mpsc::channel::<Command>(32);

    // spawn single writer task
    tokio::spawn(async move {
        // open DB inside the writer
        let mut db = Database::open("flux.wal").expect("failed to open database");

        let mut tick = tokio::time::interval(std::time::Duration::from_millis(5));
        let mut pending: Vec<PendingWrite> = Vec::new();

        // serialized execution loop
        loop {
            tokio::select! {
                Some(cmd) = rx.recv() => {
                    match cmd {
                        Command::Set { key, value, resp } => match db.put(key, value) {
                            Ok(event) => {
                                pending.push(PendingWrite { event, resp });
                            }
                            Err(e) => {
                                let _ = resp.send(Err(e.to_string()));
                            }
                        },

                        Command::Get { key, resp } => {
                            let result = db.get(&key).cloned();
                            let _ = resp.send(result);
                        }

                        Command::Del { key, resp } => match db.delete(&key) {
                            Ok(event) => {
                                pending.push(PendingWrite { event, resp });
                            }
                            Err(e) => {
                                let _ = resp.send(Err(e.to_string()));
                            }
                        },

                        Command::Patch { key, delta, resp } => match db.patch(&key, delta) {
                            Ok(event) => {
                                pending.push(PendingWrite { event, resp });
                            }
                            Err(e) => {
                                let _ = resp.send(Err(e.to_string()));
                            }
                        },
                    }
                }

                // ----- fsync batch tick -----
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
    });

    // loop to receive commands from the user
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;

        let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();

        match parts.as_slice() {
            ["SET", key, json] => {
                let value: Value = match serde_json::from_str(json) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Invalid JSON {}", e);
                        continue;
                    }
                };

                let (resp_tx, resp_rx) = oneshot::channel();

                tx.send(Command::Set {
                    key: key.to_string(),
                    value,
                    resp: resp_tx,
                })
                .await
                .expect("Failed to send command");

                match resp_rx.await {
                    Ok(result) => println!("{:?}", result),
                    Err(_) => println!("writer dropped"),
                }
            }

            ["GET", key] => {
                let (resp_tx, resp_rx) = oneshot::channel();

                tx.send(Command::Get {
                    key: key.to_string(),
                    resp: resp_tx,
                })
                .await
                .expect("Failed to send command");

                match resp_rx.await {
                    Ok(Some(doc)) => println!("{:?}", doc),
                    Ok(None) => println!("nil"),
                    Err(_) => println!("writer dropped"),
                }
            }

            ["EXIT"] => break,

            _ => println!("Unknown command"),
        }
    }

    Ok(())
}
