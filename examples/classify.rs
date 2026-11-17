#[path = "../src/classification.rs"]
mod classification;
#[path = "../src/text_model.rs"]
mod text_model;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let r: serde_json::Value = serde_json::from_str(&line).unwrap();
        let title = r["title"].as_str().unwrap_or("");
        let body = r["body"].as_str().unwrap_or("");
        let url = r["url"].as_str().unwrap_or("");
        let p = text_model::predict(title, body);
        let category = classification::classify(title, body, url, &[], &[], "");
        println!(
            "{}",
            serde_json::json!({"id":r["id"],"category":category,"reliable":classification::has_reliable_subject(title,body,url,category),"scope":p.scope,"scope_margin":p.scope_margin,"topic":p.topic,"topic_margin":p.topic_margin})
        );
    }
}
