#[allow(dead_code)]
use fluxdb::engine;

use serde_json::Value;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> io::Result<()> {
    let runtime = engine::runtime::EngineRuntime::start();
    let handler = runtime.handle;
    // loop to receive commands from the user
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;

        let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();

        match parts.as_slice() {
            ["set", key, json] => {
                let value: Value = match serde_json::from_str(json) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Invalid JSON {}", e);
                        continue;
                    }
                };

                println!("{:?}", handler.set(key.to_string(), value).await);
            }

            ["get", key] => match handler.get(key.to_string()).await {
                Ok(Some(doc)) => println!("{:?}", doc),
                Ok(None) => println!("nil"),
                Err(e) => println!("{}", e),
            },

            ["del", key] => {
                println!("{:?}", handler.delete(key.to_string()).await);
            }

            ["patch", key, json] => {
                let delta: Value = match serde_json::from_str(json) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Invalid JSON {}", e);
                        continue;
                    }
                };

                println!("{:?}", handler.patch(key.to_string(), delta).await);
            }

            ["snapshot"] => {
                println!("{:?}", handler.snapshot().await);
            }

            ["EXIT"] => break,

            _ => println!("Unknown command"),
        }
    }

    Ok(())
}
