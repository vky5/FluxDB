#![allow(dead_code)]

mod engine;
mod event;
mod interface;
mod reactivity;
mod store;

use interface::Command;
use serde_json::Value;
use std::io::{self, Write};
use tokio::sync::{mpsc, oneshot};


// Represents a write that has been executed
// but is waiting for the durability barrier (fsync)
// before the client can be acknowledged.

#[tokio::main]
async fn main() -> io::Result<()> {
    // create command queue
    let (tx, rx) = mpsc::channel::<Command>(32);

    // spawn single writer task
    tokio::spawn(engine::run_loop::run_single_writer_loop(rx));

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

            ["SNAPSHOT"] => {
                let (resp_tx, resp_rx) = oneshot::channel();

                tx.send(Command::Snapshot { resp: resp_tx })
                    .await
                    .expect("Failed to send command");

                match resp_rx.await {
                    Ok(result) => println!("{:?}", result),
                    Err(_) => println!("writer dropped"),
                }
            }

            ["EXIT"] => break,

            _ => println!("Unknown command"),
        }
    }

    Ok(())
}


