use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use fluxdb::engine::db::Database;
use fluxdb::store::kv::Store;

#[tokio::test]
async fn test_wal_torn_tail_recovery() {
    let test_dir = "./test_wal_corruption";
    if fs::metadata(test_dir).is_ok() {
        fs::remove_dir_all(test_dir).unwrap();
    }
    fs::create_dir_all(test_dir).unwrap();

    let store = Arc::new(RwLock::new(Store::new()));
    
    // 1. Initial writes
    {
        let mut db = Database::open(test_dir, store.clone()).await.unwrap();
        db.put("key1".to_string(), json!({"val": 1})).await.unwrap();
        db.put("key2".to_string(), json!({"val": 2})).await.unwrap();
        db.fsync_wal().unwrap();
    } // DB closed

    // 2. Corrupt the WAL file (append partial length prefix)
    let wal_file_path = format!("{}/wal/0.log", test_dir);
    {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&wal_file_path)
            .unwrap();
        // Just 2 bytes of a 4-byte length prefix
        file.write_all(&[0u8, 0u8]).unwrap();
        file.sync_all().unwrap();
    }

    // 3. Re-open DB - should not panic and should recover key1, key2
    {
        let _db = Database::open(test_dir, store.clone()).await.unwrap();
        let guard = store.read().await;
        assert_eq!(guard.get("key1").unwrap().value, json!({"val": 1}));
        assert_eq!(guard.get("key2").unwrap().value, json!({"val": 2}));
    }

    // 4. Corrupt further (append valid length but invalid JSON)
    {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&wal_file_path)
            .unwrap();
        let bad_json = b"{\"corrupt\": true"; // missing closing brace
        let len = bad_json.len() as u32;
        file.write_all(&len.to_be_bytes()).unwrap();
        file.write_all(bad_json).unwrap();
        file.sync_all().unwrap();
    }

    // 5. Re-open again
    {
        let _db = Database::open(test_dir, store.clone()).await.unwrap();
        let guard = store.read().await;
        assert_eq!(guard.get("key1").unwrap().value, json!({"val": 1}));
        assert_eq!(guard.get("key2").unwrap().value, json!({"val": 2}));
    }

    fs::remove_dir_all(test_dir).unwrap();
}
