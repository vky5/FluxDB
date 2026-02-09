#![allow(dead_code)]

mod command;
mod db;
mod snapshot;
mod store;

use command::Command;
use std::io::{self};
use tokio::sync::{mpsc, oneshot};
use serde_json::json;

use db::Database;
#[tokio::main]
async fn main() -> io::Result<()> {
    // create command queue
    let (tx, mut rx) = mpsc::channel::<Command>(32);

    // spawn single writer task
    tokio::spawn(async move {
        // open DB inside the writer
        let mut db = Database::open("flux.wal").expect("failed to open database");

        // serialized execution loop
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::Set { key, value, resp } => {
                    let result = db.put(key, value).map_err(|e| e.to_string());
                    let _ = resp.send(result);
                }

                Command::Get { key, resp } => {
                    let result = db.get(&key).cloned();
                    let _ = resp.send(result);
                }

                Command::Del { key, resp } => {
                    let result = db.delete(&key).map_err(|e| e.to_string());
                    let _ = resp.send(result);
                }

                Command::Patch { key, delta, resp } => {
                    let result = db.patch(&key, delta).map_err(|e| e.to_string());
                    let _ = resp.send(result);
                }
            }
        }
    });

    // create response channel
    let (resp_tx, resp_rx) = oneshot::channel();

    // send SET command into queue
    tx.send(Command::Set {
        key: "x".to_string(),
        value: json!("red"),
        resp: resp_tx,
    })
    .await
    .expect("failed to send command");

    // wait for DB response
    let result = resp_rx.await.expect("writer task dropped");

    println!("SET result: {:?}", result);

    Ok(())
}
