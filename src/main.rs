mod store;

use crate::store::wal::Wal;
use crate::store::event::Event;
use serde_json::json;

fn main() -> std::io::Result<()> {
    // 1. open WAL
    let mut wal = Wal::open("flux.wal")?;

    // 2. create a fake event
    let event = Event {
        key: "user:1".to_string(),
        old: json!(null),
        new: json!({"name": "Alice"}),
        version: 1,
    };

    // 3. append event
    wal.append(&event)?;

    println!("WAL append successful");

    println!("the event is {:?}", event);
    
    let events = Wal::replay("flux.wal")?;
 
     println!("Replayed {} events", events.len());
 
     for e in events {
         println!("{:?}", e);
     }
    

    Ok(())
}
