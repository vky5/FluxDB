use tokio::sync::oneshot;
use crate::event::Event;

pub struct PendingWrite {
    pub event: Event,
    pub resp: oneshot::Sender<Result<(), String>>,
}