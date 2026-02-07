#![allow(dead_code)]

mod db;
mod snapshot;
mod store;

use std::io::{self, Write};

use db::Database;
fn main() -> std::io::Result<()> {
    // open the database
    let mut db = Database::open("flux.wal")?;

    println!("Simple DB CLI. Commands: PUT key json | GET key | EXIT");

    loop {
        print!(">");
        io::stdout().flush()?; // show prompt
        let mut line = String::new();
        let bytes = io::stdin().read_line(&mut line)?;

        if bytes == 0 {
            println!("EOF. Exiting.");
            break;
        }

        let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();

        match parts.as_slice() {
            ["SET", key, json] => {
                let value: serde_json::Value = serde_json::from_str(json).expect("invalid JSON");

                db.put((*key).to_string(), value)?;
                println!("OK");
            }

            ["GET", key] => match db.get(key) {
                Some(doc) => println!("{:?}", doc),
                None => println!("(nil)"),
            },

            ["DEL", key] => {
                db.delete(key)?;
                println!("OK");
            }

            ["PATCH", key, json] => match serde_json::from_str::<serde_json::Value>(json) {
                Ok(delta) => {
                    db.patch(key, delta)?;
                    println!("OK");
                }
                Err(_) => println!("Invalid JSON"),
            },
            ["CHECKPOINT"] => {
                db.checkpoint("flux.wal")?;
                println!("Checkpoint created");
            }

            ["EXIT"] => break,

            _ => println!("Unknown command"),
        }
    }

    Ok(())
}
