#![allow(dead_code)]

mod engine;
mod event;
mod interface;
mod reactivity;
mod store;

use interface::Command;
use serde_json::Value;
use std::io::{self, Write};
use tokio::sync::mpsc;

use crate::engine::handler::HandlerEnum;

// Represents a write that has been executed
// but is waiting for the durability barrier (fsync)
// before the client can be acknowledged.

#[tokio::main]
async fn main() -> io::Result<()> {
    // create command queue
    let (tx, rx) = mpsc::channel::<Command>(32);

    // spawn single writer task
    tokio::spawn(engine::run_loop::run_single_writer_loop(rx));
    let handler = HandlerEnum::new(tx);

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

                println!("{:?}", handler.set(key.to_string(), value).await);
            }

            ["GET", key] => match handler.get(key.to_string()).await {
                Ok(Some(doc)) => println!("{:?}", doc),
                Ok(None) => println!("nil"),
                Err(e) => println!("{}", e),
            },

            ["DEL", key] => {
                println!("{:?}", handler.delete(key.to_string()).await);
            }

            ["PATCH", key, json] => {
                let delta: Value = match serde_json::from_str(json) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Invalid JSON {}", e);
                        continue;
                    }
                };

                println!("{:?}", handler.patch(key.to_string(), delta).await);
            }

            ["SNAPSHOT"] => {
                println!("{:?}", handler.snapshot().await);
            }

            ["EXIT"] => break,

            _ => println!("Unknown command"),
        }
    }

    Ok(())
}
