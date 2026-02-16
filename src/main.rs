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
