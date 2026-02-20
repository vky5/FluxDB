use std::fs::{File, OpenOptions};
use std::io::{self, Read, SeekFrom};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use crate::event::Event;

pub struct Segment {
    pub id: u64,
    dir: PathBuf,
    file: File,
}

impl Segment {
    pub fn create<P: AsRef<Path>>(dir: P, id: u64) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let path = dir.join(format!("{}.log", id));

        let file = OpenOptions::new()
            .create_new(true) // fail if exists
            .write(true)
            .read(true)
            .open(&path)?;

        Ok(Self { id, dir, file })
    }

    pub fn open<P: AsRef<Path>>(dir: P, id: u64) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let path = dir.join(format!("{}.log", id));

        let file = OpenOptions::new().write(true).read(true).open(&path)?;

        Ok(Self { id, dir, file })
    }

    pub fn append(&mut self, event: &Event) -> std::io::Result<u64> {
        let bytes = serde_json::to_vec(event).expect("event serialization must not fail");

        let len = bytes.len() as u32; // store the bytes in u32 number (4 bytes)

        self.file.seek(SeekFrom::End(0))?; // making sure pointer is at the nd at the time of appending 

        // Capture starting offset (LSN offset part)
        let start_offset = self.file.stream_position()?;
        self.file.write_all(&len.to_be_bytes())?;

        self.file.write_all(&bytes)?; // modifying the page of the file object from segment struct, making it dirty and then flusing it using self.fsync command later

        Ok(start_offset)

        /*
        write_all means keeps on writing bytes until all bytes are written
        whether write_all appends the file or overwrites it depends oin how the file is opened
        in our case we opened the file with append therefore it will overwrite the file

        after write_all the data is in the program's page and not written in the file
        sync_all dos this is block program until os confirms all previous writes have been flushed to a stable storage disk

        TODO batch multiple writes | fsync once per batch | see about sync_data vs sync_all
        */
    }

    pub fn fsync(&mut self) -> io::Result<()> {
        // durability barrier
        self.file.sync_all() // OS fsync api call // flush the OS page cache to disk
    }

    // move the pointer of the file to the end
    pub fn seek(&mut self, offset: u64) -> std::io::Result<()> {
        self.file.seek(std::io::SeekFrom::Start(offset))?;
        Ok(())
    }

    pub fn size(&self) -> std::io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    // read only a single event at a time (the offset is controlled by the wal.rs)
    pub fn read_next(&mut self) -> io::Result<Option<Event>> {
        // ---- 1. Read 4-byte length prefix ----
        let mut len_buf = [0u8; 4];

        match self.file.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Clean EOF or torn tail → safe stop
                return Ok(None);
            }
            Err(e) => return Err(e),
        }

        let len = u32::from_be_bytes(len_buf) as usize;

        // ---- 2. Read payload bytes ----
        let mut data = vec![0u8; len];

        match self.file.read_exact(&mut data) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Partial record at end → ignore safely
                return Ok(None);
            }
            Err(e) => return Err(e),
        }

        // ---- 3. Deserialize into Event ----
        let event: Event = serde_json::from_slice(&data)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "corrupt WAL record"))?;

        // Cursor already advanced by read_exact
        Ok(Some(event))
    }
}
