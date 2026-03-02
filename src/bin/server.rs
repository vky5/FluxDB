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
    let runtime = EngineRuntime::start();
    let handle = runtime.handle;

    let listener = TcpListener::bind("127.0.0.1:7000").await?;
    println!("server listening on 127.0.0.1:7000");

    loop {
        let (stream, addr) = listener.accept().await?;
        let handle = handle.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, handle).await {
                eprintln!("connection {addr} closed with error: {e}");
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    handle: EngineHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let (out_tx, mut out_rx) = mpsc::channel::<Response>(OUTBOUND_BUFFER);

    let writer_task = tokio::spawn(async move {
        while let Some(resp) = out_rx.recv().await {
            let line = match serde_json::to_string(&resp) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("response serialization failed: {e}");
                    break;
                }
            };

            if write_half.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if write_half.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });

    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(trimmed) {
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
                let resp = match handle.set(key, value).await {
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
