use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
};

use fluxdb::{
    engine::{handler::EngineHandle, runtime::EngineRuntime},
    net::protocol::{Request, Response},
};

const OUTBOUND_BUFFER: usize = 128;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Starting the DB engine
    let runtime = EngineRuntime::start(); // internal worker threads
    let handle = runtime.handle; // api to talk to engine

    // creating the tcp listener at port 7000
    let listener = TcpListener::bind("127.0.0.1:7000").await?;
    println!("server listening on 127.0.0.1:7000");

    loop {
        let (stream, addr) = listener.accept().await?;
        stream.set_nodelay(true)?; // Disable Nagle's algorithm for lower latency
        let handle = handle.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, handle).await {
                eprintln!("connection {addr} closed with error: {e}");
            }
        });
    }
}

// each client gets its own handle_connection  // ? this doesnt mean each request has its own connection

async fn handle_connection(
    stream: tokio::net::TcpStream,
    handle: EngineHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let (read_half, mut write_half) = stream.into_split(); // breaking tcp connection into two different handler
    let mut reader = BufReader::new(read_half);

    let (out_tx, mut out_rx) = mpsc::channel::<Response>(OUTBOUND_BUFFER);

    /*
                    ┌────────────────────┐
    Read Loop  ───▶ │                    │
    Subscription ─▶ │   out_tx channel   │ ──▶ Writer Task ──▶ TCP socket
    Subscription ─▶ │                    │
                    └────────────────────┘

    why mutex wont work?
    - block tasks writing for lock
    mix IO and business logic
    harder to reason 
    */

    let writer_task = tokio::spawn(async move { // moved the ownership of mut write_half to this task 
        while let Some(resp) = out_rx.recv().await {
            let mut line_bytes = match serde_json::to_vec(&resp) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("response serialization failed: {e}");
                    break;
                }
            };
            line_bytes.push(b'\n');

            if write_half.write_all(&line_bytes).await.is_err() { 
                break;
            }
        }
    });

    let mut line: String = String::new();

    loop {
        line.clear(); 
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed: &str = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // trimmed here is already a &str reference from line.trim() that's why no need to pass double refernece
        let req: Request = match serde_json::from_str(trimmed) { // deserialize this trimmed String back to the rust enum 
            Ok(req) => req,
            Err(e) => {
                let _ = out_tx
                    .send(Response::Error {
                        message: format!("invalid request json: {e}"),
                    })
                    .await;
                continue;
            }
        };

        match req {
            Request::Set { key, value } => {
                let resp: Response = match handle.set(key, value).await {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { message },
                };
                let _ = out_tx.send(resp).await;
            }
            Request::Get { key } => {
                let resp = match handle.get(key).await {
                    Ok(doc) => Response::Value { doc },
                    Err(message) => Response::Error { message },
                };
                let _ = out_tx.send(resp).await;
            }
            Request::Del { key } => {
                let resp = match handle.delete(key).await {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { message },
                };
                let _ = out_tx.send(resp).await;
            }
            Request::Patch { key, delta } => {
                let resp = match handle.patch(key, delta).await {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { message },
                };
                let _ = out_tx.send(resp).await;
            }
            Request::Snapshot => {
                let resp = match handle.snapshot().await {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { message },
                };
                let _ = out_tx.send(resp).await;
            }
            Request::Subscribe { key } => match handle.subscribe(key.clone()).await {
                Ok(mut sub_rx) => {
                    let _ = out_tx.send(Response::Subscribed { key }).await;

                    let sub_tx = out_tx.clone();
                    tokio::spawn(async move {
                        while let Some(event) = sub_rx.recv().await {
                            if sub_tx.send(Response::Event { event }).await.is_err() {
                                break;
                            }
                        }
                    });
                }
                Err(message) => {
                    let _ = out_tx.send(Response::Error { message }).await;
                }
            },
        }
    }

    drop(out_tx);
    let _ = writer_task.await;
    Ok(())
}

/*
* 1. Start the TCP connection at port 7000
* 2. Accept TCP connections
* 3. Spawns one async task per connection
* 4. Inside Connection:
 * A. Read JSON Requests
 * B. Send them to engine
 * C. Sends the responses back
 * D. Supports subscription (streaming events)
*/
