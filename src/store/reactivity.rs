use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::store::event::Event;
use crate::store::subscriber::Subscriber;

#[derive(Debug)]
pub struct Reactivity {
    next_id: u64, // the next id
    pub subscriptions: HashMap<String, Vec<Subscriber>>, // this is the hash map of the string (keys, and those who subscribed it )
}

impl Reactivity {
    // for adding new subscriber to the subscrptions hash map
    pub fn new() -> Self {
        // want it to return a blank Reactivity with empty subscription
        Self {
            next_id: 0,
            subscriptions: HashMap::new(),
        }
    }

    // to get the next subscriber id
    pub fn next_subscriber_id(&mut self)-> u64 {
        let id = self.next_id;
        self.next_id+=1;
        id
    }

    // Subscriber
    /*
    steps are:
        1. Create a new subscriber and receiver 
        2. Taking the key from the user see if the key is even subscribed 
        3. if it has been add the new subscriber in that vector and return the receiver
        4. if it has not been subscribed, add a new vector and key in that hashmap and return the receiver 
    */
    pub fn subscribe(&mut self, key: &str)-> mpsc::Receiver<Event> {
        // creating a new subcscriber and receiver 
        let (tx, rx) = mpsc::channel::<Event>(100);

        let subscriber: Subscriber =  Subscriber {
            id: self.next_subscriber_id(),
            tx,
        };

        self.subscriptions
            .entry(key.to_string())
            .or_default()
            .push(subscriber);


        rx
    }

    // Dispatch Event
    /*
    Dispatch is to be called for a key and event is to be sent from the dispatch. For example key 1 has some changes
    then in the end of the kv.rs after making changes , call the dispatcher with the key that has changes, and including the event
    */

    pub async fn dispatch_event(&mut self, key: &str, event: Event) {
        // clone the subscriber list 
        let subs = match self.subscriptions.get(key) {
            Some(list) => list.clone(),
            None=> return,
        };

        let mut dead_ids = Vec::new();

        for sub in subs {
            let send_result = sub.tx.send(event.clone()).await;

            if send_result.is_err() {
                dead_ids.push(sub.id);
            }
        }

        // clean up dead subscribers
        if let Some(list) = self.subscriptions.get_mut(key){
            list.retain(|sub| !dead_ids.contains(&sub.id))
        }
    }


}
