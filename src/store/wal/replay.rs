use std::{io, path::PathBuf};

use crate::{
    event::Event,
    store::wal::{lsn::Lsn, segment::Segment, wal::Wal},
};

pub struct WalIterator {
    dir: PathBuf,
    current_segment: Segment,
    current_segment_id: u64,
    last_segment_id: u64,
}

impl WalIterator {
    // this will read only a single event and return, and if it is at the end, it will shift to next segment
    pub fn next_event(&mut self) -> io::Result<Option<Event>> {
        loop {
            if let Some(event) = Segment::read_next(&mut self.current_segment)? {
                return Ok(Some(event));
            }

            // EOF reached -> check if more segment exists
            if self.current_segment_id >= self.last_segment_id {
                return Ok(None);
            }

            self.current_segment_id += 1;
            self.current_segment = Segment::open(&self.dir, self.current_segment_id)?;
        }
    }
}

impl Wal {
    pub fn replay_from(&self, lsn: Lsn) -> io::Result<WalIterator> {
        // open the starting postion from the lsn (segmnet id + offset)

        let mut segment = Segment::open(&self.dir, lsn.segment)?;

        // seek to correct offset
        segment.seek(lsn.offset);

        // determine last segment
        let last_segment_id = self.active_segment_id;
        Ok(WalIterator {
            dir: self.dir.clone(),
            current_segment: segment,
            current_segment_id:lsn.segment,
            last_segment_id,
        })
    }

    pub fn replay_all(&self) -> io::Result<WalIterator> {
        self.replay_from(Lsn::ZERO)
    }
}
