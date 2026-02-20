use std::fs::create_dir_all;
use std::path::Path;

use crate::event::Event;
use crate::store::wal::lsn::Lsn;
use crate::store::wal::segment::Segment;

pub struct Wal {
    pub dir: std::path::PathBuf, // because diferent os has different reprsentation strategy // Buf here means growable buffer
    active_segment: Segment,
    pub active_segment_id: u64,
    next_segment_id: u64,
    segment_size_limit: u64,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(dir: P, segment_size_limit: u64) -> std::io::Result<Self> {
        let dir = dir.as_ref().to_path_buf(); // b4 to_path_buf() it is of type &Path but after that it means give me the owned copy of the path

        create_dir_all(&dir)?;

        // Finding existing segments
        let mut segment_ids = vec![];
        //? OsString is not string, in unix the filenames are arbitary bytes whereas in windows it is utf 16 so the to_string_lossy converts them to utf 8 format

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy(); // get the utf 8 format for file, if valid borrow and if noat valid it is owned that's why it is returning cow enum

            if let Some(id) = name.strip_suffix(".log") {
                // Check if this is file with log prefix
                if let Ok(parsed) = id.parse::<u64>() {
                    // if the name "12" can be extracted into u64, then append it
                    segment_ids.push(parsed);
                }
            }
        }

        segment_ids.sort_unstable(); // sort but doenst maintain the order of equal thingiys // use less memory and is faster 

        let active_segment_id = segment_ids.last().copied().unwrap_or(0); // .copied here beacuse our struct stores the owned version not borrow and the .last() returns Options enum <u64>
        let next_segment_id = active_segment_id + 1;

        let active_segment = Segment::open(&dir, active_segment_id)?;

        Ok(Self {
            dir,
            active_segment,
            active_segment_id,
            next_segment_id,
            segment_size_limit,
        })
    }

    pub fn append(&mut self, event: &Event) -> std::io::Result<Lsn> {
        let payload = serde_json::to_vec(event).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "serialization failed")
        })?;

        let record_size = 4 + payload.len() as u64;

        let current_size = self.active_segment.size()?;

        if current_size + record_size > self.segment_size_limit {
            self.rotate()?;
        }

        // appending to the segment structed linked to wal
        let offset = self.active_segment.append(event)?;
        Ok(Lsn {
            segment: self.active_segment_id,
            offset,
        })
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.active_segment.fsync()?; // cant access this api after moving to new segment

        let new_id = self.next_segment_id;
        self.next_segment_id += 1;

        self.active_segment = Segment::create(&self.dir, new_id)?;

        self.active_segment_id = new_id;

        Ok(())
    }

    // pub fn replay<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<Event>> {
    //     // TODO shift the replay logic to send stream of events instead of a vecctor because of size constraints and db design schema
    //     let path: &Path = path.as_ref();

    //     // open wal file for reading
    //     let mut file = OpenOptions::new().read(true).open(path)?;

    //     let mut events = Vec::new();

    //     loop {
    //         // read length prefix (4 bytes)
    //         let mut len_buf = [0u8; 4];

    //         // if we cant read 4 bytes -> EOF or partial state -> stop
    //         if file.read_exact(&mut len_buf).is_err() {
    //             break; // not throwing error because earlier records might be useful
    //         }

    //         let len = u32::from_be_bytes(len_buf) as usize;

    //         // read payload
    //         let mut data = vec![0u8; len];
    //         if file.read_exact(&mut data).is_err() {
    //             break; // partial record -> stop safely
    //         }

    //         let event: Event = serde_json::from_slice(&data).expect("corrupt WAL record");

    //         events.push(event);
    //     }

    //     Ok(events)
    // }

    // pub fn current_offset(&mut self) -> std::io::Result<u64> {
    //     // this is not a pure read function it moves the cursor to the offset position that's why self mut
    //     self.file.seek(std::io::SeekFrom::End(0)) // move the pointer position to the end of the tape (file) and see the current position
    // }

    // pub fn replay_from(&mut self, offset: u64) -> std::io::Result<Vec<Event>> {
    //     let mut events: Vec<Event> = Vec::new();

    //     self.file.seek(SeekFrom::Start(offset))?; // move the cursor to the snapshot boundary

    //     loop {
    //         // read length prefix (4 bytes)
    //         let mut len_buf = [0u8; 4];

    //         // if we cant read 4 bytes -> EOF or partial state -> stop
    //         if self.file.read_exact(&mut len_buf).is_err() {
    //             break; // not throwing error because earlier records might be useful
    //         }

    //         let len = u32::from_be_bytes(len_buf) as usize;

    //         // read the payload
    //         let mut data = vec![0u8; len];
    //         if self.file.read_exact(&mut data).is_err() {
    //             break;
    //         }

    //         // Deserialize event
    //         let event = serde_json::from_slice(&data).expect("Corrupt wal record");
    //         events.push(event);
    //     }

    //     Ok(events)
    // }
}
