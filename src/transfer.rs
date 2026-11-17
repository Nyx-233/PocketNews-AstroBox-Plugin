// SPDX-License-Identifier: AGPL-3.0-only
// Adapted from Canopus-Module-Corona 37fba5d, docs/interconnect_proto.md.
// Modified for Pocket News: Rust sender, one news.json file, durable news commit.
use crate::news::{Article, hash};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
pub const PROTOCOL: &str = "corona-interconnect/2";
pub const PACKAGE: &str = "com.smalljelly.news";
pub const MAX_BYTES: usize = 524288;
pub use crate::news::MAX_ARTICLES;
pub const MAX_TEXT_CHARS: usize = 18000;
pub const MAX_WINDOW: usize = 4;
pub const CHUNK_BYTES: usize = 12000;
// AstroBox core's Corona sender: 8 s per acknowledgement, four attempts per packet.
pub const ACK_TIMEOUT_MS: u64 = 8000;
pub const COMMIT_TIMEOUT_MS: u64 = 120000;
pub const MAX_ATTEMPTS: u8 = 4;
const ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: String,
    pub pushed_at: u64,
    pub articles: Vec<Article>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub protocol: String,
    pub address: String,
    pub session: String,
    pub batch_id: String,
    pub payload: String,
    pub messages: Vec<String>,
    pub current: usize,
    pub retries: u8,
    pub paused: bool,
    pub manifest_ready: bool,
    pub chunk_bytes: usize,
    pub window: usize,
    #[serde(skip)]
    pub timer: Option<u64>,
    #[serde(skip)]
    pub generation: u64,
    #[serde(skip)]
    pub pumping: bool,
    #[serde(skip)]
    pub run: u64,
    #[serde(skip)]
    acknowledged: BTreeSet<usize>,
    #[serde(skip)]
    sent: BTreeSet<usize>,
}
#[derive(Debug)]
pub enum Reply {
    Ignored,
    Next,
    Complete,
    Error(String),
}

pub fn control(kind: &str, fields: Value) -> String {
    format!("{kind}{fields}")
}
pub fn encode_base91(bytes: &[u8]) -> String {
    let (mut accumulator, mut bits) = (0u32, 0u32);
    let mut out = String::new();
    for &byte in bytes {
        accumulator |= (byte as u32) << bits;
        bits += 8;
        if bits > 13 {
            let mut value = accumulator & 8191;
            if value > 88 {
                accumulator >>= 13;
                bits -= 13;
            } else {
                value = accumulator & 16383;
                accumulator >>= 14;
                bits -= 14;
            }
            out.push(ALPHABET[(value % 91) as usize] as char);
            out.push(ALPHABET[(value / 91) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[(accumulator % 91) as usize] as char);
        if bits > 7 || accumulator > 90 {
            out.push(ALPHABET[(accumulator / 91) as usize] as char);
        }
    }
    out
}
pub fn decode_message(raw: &str) -> Option<Value> {
    fn packet(text: &str) -> Option<Value> {
        if text.chars().count() > MAX_TEXT_CHARS {
            return None;
        }
        let kind = text.get(..1)?;
        match kind {
            "H" | "T" | "P" | "E" => {
                let mut value: Value = serde_json::from_str(text.get(1..)?).ok()?;
                value.as_object_mut()?.insert("kind".into(), json!(kind));
                Some(value)
            }
            "A" if text.is_ascii() && text.len() == 9 => Some(
                json!({"kind":"A", "fileIndex":u16::from_str_radix(&text[1..5],16).ok()?, "chunkIndex":u16::from_str_radix(&text[5..9],16).ok()?}),
            ),
            "C" if text.is_ascii() && text.len() == 5 => {
                Some(json!({"kind":"C", "fileIndex":u16::from_str_radix(&text[1..5],16).ok()?}))
            }
            _ => None,
        }
    }
    if let Some(value) = packet(raw) {
        return Some(value);
    }
    let mut value: Value = serde_json::from_str(raw).ok()?;
    for _ in 0..8 {
        if let Some(text) = value.as_str() {
            if let Some(packet) = packet(text) {
                return Some(packet);
            }
            value = serde_json::from_str(text).ok()?;
        } else if let Some(bytes) = value.as_array() {
            let bytes: Option<Vec<u8>> = bytes
                .iter()
                .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect();
            let bytes = bytes?;
            let text = std::str::from_utf8(&bytes).ok()?;
            if let Some(packet) = packet(text) {
                return Some(packet);
            }
            value = serde_json::from_str(text).ok()?;
        } else {
            value = value
                .get("msg")
                .or_else(|| value.get("data"))
                // The host event contains payloadText; the quick app uses msg inside it.
                .or_else(|| value.get("payloadText"))
                .or_else(|| value.get("payload"))?
                .clone();
        }
    }
    None
}
impl Transfer {
    pub fn new(address: String, articles: Vec<Article>, now: u64) -> Result<Self, String> {
        if articles.is_empty() || articles.len() > MAX_ARTICLES {
            return Err(format!("每批请选择 1–{MAX_ARTICLES} 篇新闻"));
        }
        let batch_id = format!("batch_{now}");
        let payload = serde_json::to_string(&Batch {
            id: batch_id.clone(),
            pushed_at: now,
            articles,
        })
        .map_err(|e| e.to_string())?;
        if payload.len() > MAX_BYTES {
            return Err("整批内容超过512 KB，请减少篇数后推送，正文不会被截断".into());
        }
        let mut task = Self {
            protocol: PROTOCOL.into(),
            session: format!("session_{now}_{}", hash(&address)),
            address,
            batch_id,
            payload,
            messages: vec![],
            current: 0,
            retries: 0,
            paused: false,
            manifest_ready: false,
            chunk_bytes: CHUNK_BYTES,
            window: MAX_WINDOW,
            timer: None,
            generation: 0,
            pumping: false,
            run: 0,
            acknowledged: BTreeSet::new(),
            sent: BTreeSet::new(),
        };
        task.build_messages();
        Ok(task)
    }
    fn build_messages(&mut self) {
        let id = &self.batch_id;
        let count = self.payload.len().div_ceil(self.chunk_bytes);
        self.messages = vec![
            control(
                "T",
                json!({"operation":"begin", "themeId":id, "mode":if self.manifest_ready {"resume"} else {"replace"}, "fileCount":1, "totalBytes":self.payload.len()}),
            ),
            control(
                "T",
                json!({"operation":"file", "themeId":id, "fileIndex":0, "relativePath":"news.json", "sizeBytes":self.payload.len()}),
            ),
            control("T", json!({"operation":"end", "themeId":id})),
            control(
                "P",
                json!({"themeId":id,"fileIndex":0,"sizeBytes":self.payload.len(),"chunkSizeBytes":self.chunk_bytes,"chunkCount":count}),
            ),
        ];
        for (i, bytes) in self.payload.as_bytes().chunks(self.chunk_bytes).enumerate() {
            self.messages
                .push(format!("F0000{i:04x}{}", encode_base91(bytes)));
        }
        self.messages.push("C0000".into());
        self.messages
            .push(control("T", json!({"operation":"finish","themeId":id})));
    }
    pub fn restart(&mut self) {
        self.run += 1;
        self.current = 0;
        self.retries = 0;
        self.paused = false;
        self.pumping = false;
        self.sent.clear();
        self.acknowledged.clear();
        self.generation += 1;
        self.build_messages();
    }
    pub fn handshake_id(&self) -> String {
        format!("{}_{}", self.session, self.run)
    }
    pub fn handshake(&self) -> String {
        control(
            "H",
            json!({"version":2,"type":"request","requestId":self.handshake_id(),"themeId":self.batch_id,"maxTextChars":MAX_TEXT_CHARS}),
        )
    }
    pub fn negotiate(&mut self, msg: &Value) -> Result<(), String> {
        let max = msg["maxTextChars"]
            .as_u64()
            .filter(|n| (256..=MAX_TEXT_CHARS as u64).contains(n))
            .ok_or("无效互联消息长度")? as usize;
        let window = msg["maxWindow"]
            .as_u64()
            .filter(|n| (1..=4).contains(n))
            .ok_or("无效互联窗口")? as usize;
        if msg["version"] != 2 {
            return Err("请同时更新手环应用和插件".into());
        }
        // F header is 9 chars; Corona's sender reserves two more for basE91 rounding.
        let chunk = CHUNK_BYTES.min((max - 11) * 13 / 16);
        if self.manifest_ready && self.chunk_bytes > chunk {
            return Err("连接容量变化，请放弃此批后重新推送".into());
        }
        if !self.manifest_ready {
            self.chunk_bytes = chunk;
        }
        self.window = window;
        self.build_messages();
        Ok(())
    }
    fn data_end(&self) -> usize {
        self.messages.len() - 2
    }
    fn is_data(&self) -> bool {
        self.current >= 4 && self.current < self.data_end()
    }
    pub fn next_packet(&mut self) -> Option<(usize, String)> {
        if self.paused {
            return None;
        }
        let index = if self.is_data() {
            if self.sent.len() >= self.window {
                return None;
            }
            (4..self.data_end())
                .find(|i| !self.sent.contains(i) && !self.acknowledged.contains(i))?
        } else {
            if self.sent.contains(&self.current) {
                return None;
            }
            self.current
        };
        self.sent.insert(index);
        Some((index, self.messages[index].clone()))
    }
    pub fn retry_pending(&mut self) -> bool {
        if self.retries + 1 >= MAX_ATTEMPTS {
            return false;
        }
        self.retries += 1;
        self.sent.clear();
        true
    }
    pub fn acknowledgement_timeout_ms(&self) -> u64 {
        if self.messages.len().checked_sub(1) == Some(self.current) {
            COMMIT_TIMEOUT_MS
        } else {
            ACK_TIMEOUT_MS
        }
    }
    pub fn reply(&mut self, msg: &Value) -> Reply {
        if self.paused {
            return Reply::Ignored;
        }
        // As in Corona's sender, a control packet for another theme is skipped,
        // while E or any reject status for this upload aborts the current step.
        if msg.get("themeId").is_some_and(|id| id != &self.batch_id) {
            return Reply::Ignored;
        }
        if msg["kind"] == "E" || msg["status"] == "reject" {
            let detail = msg["message"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" · {}", s.chars().take(200).collect::<String>()))
                .unwrap_or_default();
            return Reply::Error(format!(
                "手环接收失败: {}{detail}",
                msg["errorCode"].as_str().unwrap_or("rejected")
            ));
        }
        if self.is_data() && msg["kind"] == "A" && msg["fileIndex"] == 0 {
            let Some(chunk) = msg["chunkIndex"].as_u64() else {
                return Reply::Ignored;
            };
            let index = chunk as usize + 4;
            if !self.sent.remove(&index) || !self.acknowledged.insert(index) {
                return Reply::Ignored;
            }
            if self.acknowledged.len() == self.data_end() - 4 {
                self.current = self.data_end();
            }
            self.retries = 0;
            return Reply::Next;
        }
        let named = msg["themeId"] == self.batch_id;
        if (self.current == 3 || self.current == self.data_end())
            && named
            && msg["kind"] == "P"
            && msg["fileIndex"] == 0
            && self.sent.contains(&self.current)
        {
            if !matches!(
                msg["status"].as_str(),
                Some("ready" | "resume" | "complete")
            ) {
                return Reply::Error("手环拒绝了文件准备".into());
            }
            let Some(window) = msg["window"].as_u64().filter(|n| (1..=4).contains(n)) else {
                return Reply::Error("无效分片窗口".into());
            };
            let Some(ranges) = msg["receivedRanges"].as_array() else {
                return Reply::Error("无效续传范围".into());
            };
            let mut received = BTreeSet::new();
            for range in ranges {
                let Some(pair) = range.as_array().filter(|v| v.len() == 2) else {
                    return Reply::Error("无效续传范围".into());
                };
                let (Some(start), Some(end)) = (pair[0].as_u64(), pair[1].as_u64()) else {
                    return Reply::Error("无效续传范围".into());
                };
                if start > end || end >= (self.data_end() - 4) as u64 {
                    return Reply::Error("续传范围超限".into());
                }
                for i in start..=end {
                    received.insert(i as usize + 4);
                }
            }
            self.window = self.window.min(window as usize);
            self.acknowledged = received;
            self.current = if self.acknowledged.len() == self.data_end() - 4 {
                self.data_end()
            } else {
                4
            };
        } else {
            let matched = match self.current {
                0 => {
                    named
                        && msg["kind"] == "T"
                        && msg["operation"] == "ack"
                        && msg["itemType"] == "begin"
                }
                1 => {
                    named
                        && msg["kind"] == "T"
                        && msg["operation"] == "ack"
                        && msg["itemType"] == "file"
                        && msg["fileIndex"] == 0
                }
                2 => {
                    named
                        && msg["kind"] == "T"
                        && msg["operation"] == "status"
                        && msg["status"] == "ready"
                }
                i if i == self.data_end() => msg["kind"] == "C" && msg["fileIndex"] == 0,
                i if i == self.messages.len() - 1 => {
                    named
                        && msg["kind"] == "T"
                        && msg["operation"] == "status"
                        && msg["status"] == "ready"
                }
                _ => false,
            };
            if !matched || !self.sent.contains(&self.current) {
                return Reply::Ignored;
            }
            if self.messages.len().checked_sub(1) == Some(self.current) {
                return Reply::Complete;
            }
            if self.current == 2 {
                self.manifest_ready = true;
            }
            self.current += 1;
        }
        self.sent.clear();
        self.retries = 0;
        Reply::Next
    }
    pub fn percent(&self) -> usize {
        if self.current < 4 {
            0
        } else if self.current >= self.data_end() {
            100
        } else {
            self.acknowledged.len() * 100 / (self.data_end() - 4)
        }
    }
}
