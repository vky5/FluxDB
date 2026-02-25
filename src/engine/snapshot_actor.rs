use std::{
    fs::{File, rename},
    io::Write,
    path::Path,
};

use tokio::{
    sync::{mpsc, oneshot},
    time::{Duration, interval},
};

use crate::{interface::command::WriteCommand, store::snapshot::Snapshot};

pub enum SnapshotActorCommand {
    TriggerNow,
    TriggerNowWithAck {
        resp: oneshot::Sender<Result<(), String>>,
    },
}

pub async fn snapshot_actor(
    mut rx: mpsc::Receiver<SnapshotActorCommand>,
    write_tx: mpsc::Sender<WriteCommand>,
    period: Duration,
) {
    let mut tick = interval(period);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let _ = run_snapshot_cycle(&write_tx).await;
            }

            cmd = rx.recv() => {
                match cmd {
                    Some(SnapshotActorCommand::TriggerNow) => {
                        let _ = run_snapshot_cycle(&write_tx).await;
                    }

                    Some(SnapshotActorCommand::TriggerNowWithAck { resp }) => {
                        let result = run_snapshot_cycle(&write_tx).await;
                        let _ = resp.send(result);
                    }

                    None => break,
                }
            }
        }
    }
}

async fn run_snapshot_cycle(write_tx: &mpsc::Sender<WriteCommand>) -> Result<(), String> {
    let snapshot = request_snapshot_payload(write_tx).await?;
    checkpoint_durability("flux.wal", &snapshot)?;
    Ok(())
}

async fn request_snapshot_payload(
    write_tx: &mpsc::Sender<WriteCommand>,
) -> Result<Snapshot, String> {
    let (resp_tx, resp_rx) = oneshot::channel();

    write_tx
        .send(WriteCommand::Snapshot { resp: resp_tx })
        .await
        .map_err(|_| "writer dropped".to_string())?;

    resp_rx.await.map_err(|_| "writer dropped".to_string())?
}

fn checkpoint_durability(wal_path: &str, snapshot: &Snapshot) -> Result<(), String> {
    let final_path = format!("{wal_path}.snapshot");
    let tmp_path = format!("{final_path}.tmp");

    let bytes =
        serde_json::to_vec(snapshot).map_err(|e| format!("snapshot serialize error: {e}"))?;

    let mut tmp_file =
        File::create(&tmp_path).map_err(|e| format!("snapshot create tmp error: {e}"))?;
    tmp_file
        .write_all(&bytes)
        .map_err(|e| format!("snapshot write tmp error: {e}"))?;
    tmp_file
        .sync_all()
        .map_err(|e| format!("snapshot fsync tmp error: {e}"))?;

    rename(&tmp_path, &final_path).map_err(|e| format!("snapshot rename error: {e}"))?;

    let dir = Path::new(&final_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("snapshot dir fsync error: {e}"))?;

    Ok(())
}
