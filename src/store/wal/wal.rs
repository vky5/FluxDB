use std::fs::create_dir_all;
use std::io;
use std::path::Path;

use crate::event::Event;
use crate::store::wal::lsn::Lsn;
use crate::store::wal::segment::Segment;

pub struct Wal {
    pub dir: std::path::PathBuf, // because diferent os has different reprsentation strategy // Buf here means growable buffer
    pub active_segment: Segment,
    pub active_segment_id: u64,
    next_segment_id: u64,
    segment_size_limit: u64,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(dir: P, segment_size_limit: u64) -> std::io::Result<Self> {
        let root = dir.as_ref().to_path_buf(); // b4 to_path_buf() it is of type &Path but after that it means give me the owned copy of the path
        let dir = root.join("wal");

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
        let active_segment_id;
        let next_segment_id;

        let active_segment;
        if segment_ids.is_empty() {
            let seg = Segment::create(&dir, 0)?;
            active_segment_id = 0;
            next_segment_id = 1;
            active_segment = seg;
        } else {
            let id = *segment_ids.last().unwrap();
            let seg = Segment::open(&dir, id)?;
            active_segment_id = id;
            next_segment_id = id + 1;
            active_segment = seg;
        }

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

    pub fn current_lsn(&self) -> io::Result<Lsn> {
        let offset = self.active_segment.size()?;

        Ok(Lsn {
            segment: self.active_segment_id,
            offset,
        })
    }

    pub fn gc(&mut self, upto: Lsn) -> io::Result<()> {
        // delete all segments with id < upto.segment
        for segment_id in 0..upto.segment {
            if segment_id >= self.active_segment_id {
                continue;
            }

            let path = self.dir.join(format!("{}.log", segment_id));
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}
