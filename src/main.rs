mod store;
use store::Store;
use serde_json::json;

fn main() {
    let mut store = Store::new();

    store.put("user1".into(), json!({
        "name": "Vaibhav",
        "details": {
            "age": 20,
            "city": "Delhi"
        }
    }));

    // Update age only
    store.patch("user1", json!({
        "details": { "age": 21 }
    }));

    if let Some(doc) = store.get("user1") {
        println!("Value: {}, Version: {}", doc.value, doc.version);
    }

    store.delete("user1");
    println!("After delete: {:?}", store.data);
}
