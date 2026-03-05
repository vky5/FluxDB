use clap::{Parser, Subcommand};
use std::{io::Write, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::{mpsc, oneshot, Mutex},
};

use fluxdb::net::protocol::{Request, Response};

#[derive(Parser, Debug)] // Parser - converts command line arguments into this struct
#[command(author, version, about = "FluxDB TCP client")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:7000")]
    addr: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Set { key: String, value: String },
    Get { key: String },
    Del { key: String },
    Patch { key: String, delta: String },
    Snapshot,
    Shell,
    Subscribe { key: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Two modes:
    // - run_once: one command over one connection
    // - run_shell: persistent interactive session
    match cli.command {
        Command::Shell => run_shell(&cli.addr).await?,
        other => run_once(&cli.addr, &other).await?,
    }

    Ok(())
}

async fn run_once(addr: &str, command: &Command) -> Result<(), Box<dyn std::error::Error>> {
    let req = build_request(command)?;
    let stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Line-delimited JSON request.
    let line = serde_json::to_string(&req)?;
    write_half.write_all(line.as_bytes()).await?;
    write_half.write_all(b"\n").await?;

    match command {
        Command::Subscribe { .. } => {
            // Subscribe is a long-running stream, so keep printing until disconnect.
            let mut line = String::new();
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    break;
                }
                println!("{}", line.trim());
            }
        }
        _ => {
            let mut line = String::new();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                return Err("server closed without response".into());
            }
            let resp: Response = serde_json::from_str(line.trim())?;
            println!("{resp:?}");
        }
    }

    Ok(())
}

async fn run_shell(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    let (read_half, mut write_half) = stream.into_split();
    let mut socket_reader = BufReader::new(read_half);

    // pending holds the responder for the single in-flight command.
    // No request_id yet, so we intentionally allow one outstanding request at a time.
    let pending: Arc<Mutex<Option<oneshot::Sender<Response>>>> = Arc::new(Mutex::new(None));

    // event_tx/event_rx carries asynchronous subscription events to a dedicated printer task.
    let (event_tx, mut event_rx) = mpsc::channel::<Response>(128);

    let pending_for_reader = pending.clone();
    tokio::spawn(async move {
        // Socket reader loop: parse each server line and route it.
        let mut line = String::new();
        loop {
            line.clear();
            let n = match socket_reader.read_line(&mut line).await {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("read error: {e}");
                    break;
                }
            };
            if n == 0 {
                break;
            }

            let resp: Response = match serde_json::from_str(line.trim()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("invalid response json: {e}");
                    continue;
                }
            };

            match resp {
                Response::Event { .. } => {
                    // Events are stream messages; forward to event queue.
                    let _ = event_tx.send(resp).await;
                }
                other => {
                    // Non-event is treated as the reply for current pending command.
                    if let Some(tx) = pending_for_reader.lock().await.take() {
                        let _ = tx.send(other);
                    } else {
                        // Useful signal if server sends a non-event without a waiting request.
                        println!("unsolicited: {other:?}");
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        // Event printer loop. Runs independently so shell input stays usable.
        while let Some(ev) = event_rx.recv().await {
            println!("[event] {ev:?}");
        }
    });

    println!("shell connected to {addr}");
    println!("commands: set/get/del/patch/snapshot/subscribe, type 'exit' to quit");

    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);
    let mut input = String::new();

    loop {
        // Interactive shell loop.
        print!("> ");
        std::io::stdout().flush()?;

        input.clear();
        let n = stdin_reader.read_line(&mut input).await?;
        if n == 0 {
            break;
        }

        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("exit") {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }

        let req: Request = match parse_shell_request(trimmed) {
            Ok(r) => r,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };

        // oneshot pair = one response promise for this one command.
        let (tx, rx) = oneshot::channel();
        {
            let mut slot = pending.lock().await;
            if slot.is_some() {
                println!("request already in-flight, wait");
                continue;
            }
            *slot = Some(tx);
        }

        // Convert shell command to wire request JSON.
        let line = serde_json::to_string(&req)?;
        write_half.write_all(line.as_bytes()).await?;
        write_half.write_all(b"\n").await?;

        // Wait until socket-reader routes the matching non-event response.
        match rx.await {
            Ok(resp) => println!("{resp:?}"),
            Err(_) => {
                println!("connection closed before response");
                break;
            }
        }
    }

    Ok(())
}

// maps the subcommands to their respective requests based on protocol.rs file
fn build_request(command: &Command) -> Result<Request, Box<dyn std::error::Error>> {
    let req = match command {
        Command::Set { key, value } => Request::Set {
            key: key.clone(),
            value: serde_json::from_str(value)?,
        },
        Command::Get { key } => Request::Get { key: key.clone() },
        Command::Del { key } => Request::Del { key: key.clone() },
        Command::Patch { key, delta } => Request::Patch {
            key: key.clone(),
            delta: serde_json::from_str(delta)?,
        },
        Command::Snapshot => Request::Snapshot,
        Command::Subscribe { key } => Request::Subscribe { key: key.clone() },
        Command::Shell => {
            return Err("shell is interactive; no single request mapping".into());
        }
    };

    Ok(req)
}

fn parse_shell_request(input: &str) -> Result<Request, String> {
    let mut parts = input.splitn(2, ' ');
    let cmd = parts
        .next()
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| "empty command".to_string())?;
    let rest = parts.next().unwrap_or("").trim();

    match cmd.as_str() {
        "set" => {
            let mut p = rest.splitn(2, ' ');
            let key = p
                .next()
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "usage: set <key> <json_value>".to_string())?;
            let value_str = p.next().unwrap_or("").trim();
            if value_str.is_empty() {
                return Err("usage: set <key> <json_value>".to_string());
            }
            let value = serde_json::from_str(value_str)
                .map_err(|e| format!("invalid JSON value for set: {e}"))?;
            Ok(Request::Set {
                key: key.to_string(),
                value,
            })
        }
        "get" => {
            if rest.is_empty() {
                return Err("usage: get <key>".to_string());
            }
            Ok(Request::Get {
                key: rest.to_string(),
            })
        }
        "del" => {
            if rest.is_empty() {
                return Err("usage: del <key>".to_string());
            }
            Ok(Request::Del {
                key: rest.to_string(),
            })
        }
        "patch" => {
            let mut p = rest.splitn(2, ' ');
            let key = p
                .next()
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "usage: patch <key> <json_delta>".to_string())?;
            let delta_str = p.next().unwrap_or("").trim();
            if delta_str.is_empty() {
                return Err("usage: patch <key> <json_delta>".to_string());
            }
            let delta = serde_json::from_str(delta_str)
                .map_err(|e| format!("invalid JSON delta for patch: {e}"))?;
            Ok(Request::Patch {
                key: key.to_string(),
                delta,
            })
        }
        "snapshot" => {
            if !rest.is_empty() {
                return Err("usage: snapshot".to_string());
            }
            Ok(Request::Snapshot)
        }
        "subscribe" => {
            if rest.is_empty() {
                return Err("usage: subscribe <key>".to_string());
            }
            Ok(Request::Subscribe {
                key: rest.to_string(),
            })
        }
        _ => Err(
            "unknown command. use: set/get/del/patch/snapshot/subscribe/exit".to_string(),
        ),
    }
}
