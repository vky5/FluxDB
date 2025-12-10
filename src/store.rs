use std::collections::HashMap;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Document {
    pub value: Value,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub data: HashMap<String, Document>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: HashMap::new(),
        }
    }

    pub fn put(&mut self, key: String, value: Value) {
        let version = match self.data.get(&key) {
            Some(doc) => doc.version + 1,
            None => 1,
        };

        let new_doc = Document { value, version };

        self.data.insert(key, new_doc);
    }

    pub fn get(&self, key: &str) -> Option<&Document> {
        self.data.get(key)
    }

    pub fn delete(&mut self, key: &str){
        self.data.remove(key);
    }

    pub fn patch(&mut self, key: &str, delta: Value){
        if let Some(doc) = self.data.get_mut(key){
            merge_json(&mut doc.value, &delta);

            doc.version+=1;
        }
    }

}


fn merge_json(target: &mut Value, delta: &Value) {
    match (target, delta) {
        // If both are JSON objects → recursively merge fields
        (Value::Object(t), Value::Object(d)) => {
            for (k, v) in d {
                merge_json(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        // Otherwise, overwrite completely
        (t, d) => {
            *t = d.clone();
        }
    }
}
