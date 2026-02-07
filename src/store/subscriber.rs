use tokio::sync::mpsc;

use crate::store::event::Event;


#[derive(Debug)]
pub struct Subscriber {
    pub id: u64, // unique subscruber identifier
    pub tx: mpsc::Sender<Event>, // channel for sending events to this subscriber
}

/*
mpsc - multi producer single consumer

meaning: Many senders (tx)
one receiver (rx)

The store may have many send points (multiple tx clones)
But each subscriber has ONE receiver (their own rx)

one more thing there are not two channel one for sending and other for receiving but there is only single channel
the rust gives two handles to send and receive event

*/