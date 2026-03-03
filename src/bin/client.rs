use clap::{Parser, Subcommand};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use fluxdb::net::protocol::{Request, Response};

#[derive(Parser, Debug)] // Parser -converts command line arguments into this struct
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
    Subscribe { key: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let req = build_request(&cli.command)?;
    let stream = TcpStream::connect(&cli.addr).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let line = serde_json::to_string(&req)?;
    write_half.write_all(line.as_bytes()).await?;
    write_half.write_all(b"\n").await?;

    match cli.command {
        Command::Subscribe { .. } => {
            let mut line: String = String::new();
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
    };

    Ok(req)
}
