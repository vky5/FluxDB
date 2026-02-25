use tokio::sync::{mpsc, oneshot};

use crate::{event::Event, reactivity::reactivity::Reactivity};

pub enum NotifyCommand {
    Subscribe {
        key: String,
        resp: oneshot::Sender<mpsc::Receiver<Event>>,
    },
    Dispatch {
        event: Event,
    },
}

pub struct NotifyActor {
    reactivity: Reactivity,
    rx: mpsc::Receiver<NotifyCommand>, // the channel on which the request for notify_actor will get. The runtime owns tx of this channel
}

impl NotifyActor {
    pub fn new(rx: mpsc::Receiver<NotifyCommand>) -> Self {
        Self {
            reactivity: Reactivity::new(),
            rx,
        }
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                NotifyCommand::Subscribe { key, resp } => {
                    let sub = self.reactivity.subscribe(&key);
                    let _ = resp.send(sub);
                }
                NotifyCommand::Dispatch { event } => {
                    self.reactivity.dispatch_event(&event);
                }
            }
        }
    }
}
