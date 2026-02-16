use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::store::Event;

pub struct Wal {
    file: File,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        // this means on success return ann instance of this type
        let path = path.as_ref();
        let file = OpenOptions::new()
            .create(true) // create if missing
            .append(true) // always write at the end (otheriwse it would have replaced)
            .read(true) // needed for replay later
            .open(path)?;

        Ok(Self { file })
    }

    pub fn append(&mut self, event: &Event) -> std::io::Result<()> {
        // on success I return nothing special just confiarmation we used stdio in both of the case for the error type otherwise would be good enough with just result enum
        // serialize the event
        let bytes = serde_json::to_vec(event).expect("event serialization must not fail"); // convert json to bytes

        // write length prefix
        let len = bytes.len() as u32; // 4 bytes
        self.file.write_all(&len.to_be_bytes())?; // immutable borrow to write 

        //  write payload
        self.file.write_all(&bytes)?; // immutable borrow

        // durability barrier
        self.file.sync_all()?; // OS fsync api call

        Ok(())

        /*
        write_all means keeps on writing bytes until all bytes are written
        whether write_all appends the file or overwrites it depends oin how the file is opened
        in our case we opened the file with append therefore it will overwrite the file

        after write_all the data is in the program's page and not written in the file
        sync_all dos this is block program until os confirms all previous writes have been flushed to a stable storage disk

        TODO batch multiple writes | fsync once per batch | see about sync_data vs sync_all
        */
    }

    pub fn replay<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<Event>> {
        // TODO shift the replay logic to send stream of events instead of a vecctor because of size constraints and db design schema
        let path: &Path = path.as_ref();

        // open wal file for reading
        let mut file = OpenOptions::new().read(true).open(path)?;

        let mut events = Vec::new();

        loop {
            // read length prefix (4 bytes)
            let mut len_buf = [0u8; 4];

            // if we cant read 4 bytes -> EOF or partial state -> stop
            if file.read_exact(&mut len_buf).is_err() {
                break; // not throwing error because earlier records might be useful
            }

            let len = u32::from_be_bytes(len_buf) as usize;

            // read payload
            let mut data = vec![0u8; len];
            if file.read_exact(&mut data).is_err() {
                break; // partial record -> stop safely
            }

            let event: Event = serde_json::from_slice(&data).expect("corrupt WAL record");

            events.push(event);
        }

        Ok(events)
    }

    pub fn current_offset(&mut self) -> std::io::Result<u64> {
        // this is not a pure read function it moves the cursor to the offset position that's why self mut
        self.file.seek(std::io::SeekFrom::End(0)) // move the pointer position to the end of the tape (file) and see the current position
    }

    pub fn replay_from(&mut self, offset: u64) -> std::io::Result<Vec<Event>> {
        let mut events: Vec<Event> = Vec::new();

        self.file.seek(SeekFrom::Start(offset))?; // move the cursor to the snapshot boundary

        loop {
            // read length prefix (4 bytes)
            let mut len_buf = [0u8; 4];

            // if we cant read 4 bytes -> EOF or partial state -> stop
            if self.file.read_exact(&mut len_buf).is_err() {
                break; // not throwing error because earlier records might be useful
            }

            let len = u32::from_be_bytes(len_buf) as usize;

            // read the payload
            let mut data = vec![0u8; len];
            if self.file.read_exact(&mut data).is_err() {
                break;
            }

            // Deserialize event
            let event = serde_json::from_slice(&data).expect("Corrupt wal record");
            events.push(event);
        }

        Ok(events)
    }
}
