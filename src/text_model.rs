// Field-aware local SVMs: geographic scope first, subject second.
use std::{collections::HashMap, sync::OnceLock};
const POLICY: &str = include_str!("../training/classifier-policy.json");
const TOPIC: &[u8] = include_bytes!("classifier-model.bin");
const SCOPE: &[u8] = include_bytes!("classifier-scope.bin");
fn policy() -> &'static serde_json::Value {
    static VALUE: OnceLock<serde_json::Value> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(POLICY).expect("classifier policy"))
}
fn terms(key: &str) -> impl Iterator<Item = &'static str> {
    policy()[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
}
pub(crate) fn clean(text: &str) -> String {
    let text: String = text
        .trim()
        .chars()
        .flat_map(|c| {
            if ('！'..='～').contains(&c) {
                char::from_u32(c as u32 - 0xfee0).unwrap()
            } else {
                c
            }
            .to_lowercase()
        })
        .collect();
    text.lines()
        .filter_map(|line| {
            let mut line = line.trim();
            if terms("sources").any(|s| line.starts_with(&s.to_lowercase())) {
                if let Some((prefix, rest)) = line.split_once(',') {
                    if prefix.chars().count() <= 55
                        && (prefix.ends_with('电') || prefix.contains("消息"))
                    {
                        line = rest;
                    }
                }
            }
            (!line.is_empty()).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut number = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_digit() {
                if !number {
                    word.push('#');
                }
                number = true;
            } else {
                word.push(c);
                number = false;
            }
        } else {
            if !word.is_empty() {
                out.push(format!("@{}", std::mem::take(&mut word)));
            }
            number = false;
            if c.is_alphanumeric() {
                out.push(c.to_string());
            }
        }
    }
    if !word.is_empty() {
        out.push(format!("@{word}"));
    }
    out
}
fn position(text: &str, term: &str) -> Option<usize> {
    let term = term.to_lowercase();
    if term == "国产" {
        return text.match_indices(&term).find_map(|(i, _)| {
            (!terms("foreign_places")
                .filter(|s| s.ends_with('国'))
                .any(|s| text[..i + "国".len()].ends_with(s)))
            .then_some(i)
        });
    }
    text.match_indices(&term).find_map(|(i, _)| {
        let word = |c: u8| c.is_ascii_alphanumeric();
        (!term.is_ascii()
            || (i == 0 || !word(text.as_bytes()[i - 1]))
                && (i + term.len() == text.len() || !word(text.as_bytes()[i + term.len()])))
        .then_some(i)
    })
}
fn any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}
fn flags(text: &str) -> Vec<&'static str> {
    let keys = [
        "foreign_places",
        "foreign_actors",
        "domestic_actors",
        "domestic_places",
        "domestic_context",
        "foreign_reports",
        "guide_markers",
        "news_actions",
    ];
    let pos: Vec<_> = keys
        .iter()
        .map(|key| {
            terms(key)
                .filter_map(|s| position(text, s))
                .min()
                .unwrap_or(usize::MAX)
        })
        .collect();
    let mut out: Vec<_> = keys
        .iter()
        .zip(&pos)
        .filter_map(|(&key, &p)| (p != usize::MAX).then_some(key))
        .collect();
    let equity = [
        "*st",
        "a股",
        "沪指",
        "沪深",
        "深交所",
        "上交所",
        "科创板",
        "创业板",
    ]
    .iter()
    .filter_map(|s| text.find(s))
    .min()
    .unwrap_or(usize::MAX);
    let foreign = pos[0].min(pos[1]);
    let domestic = pos[2].min(equity);
    let fiction = any(
        text,
        &[
            "游戏中",
            "游戏里",
            "游戏玩法",
            "虚构角色",
            "虚构城市",
            "剧情",
            "小说",
        ],
    );
    let legal = any(
        text,
        &[
            "法院", "法庭", "裁定", "判决", "上诉", "起诉", "诉讼", "仲裁", "审理", "刑拘", "立案",
        ],
    );
    let finance = any(
        text,
        &[
            "财报",
            "营收",
            "净利润",
            "股价",
            "股指",
            "收益率",
            "利率",
            "汇率",
            "融资",
            "债券",
            "期货",
        ],
    );
    for topic in [
        "culture",
        "entertainment",
        "finance",
        "life",
        "society",
        "education",
        "sports",
        "tech",
        "china",
        "military",
    ] {
        if policy()["topic_hints"][topic]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| position(text, v.as_str().unwrap()).is_some())
        {
            out.push(match topic {
                "culture" => "hint_culture",
                "entertainment" => "hint_entertainment",
                "finance" => "hint_finance",
                "life" => "hint_life",
                "society" => "hint_society",
                "education" => "hint_education",
                "sports" => "hint_sports",
                "tech" => "hint_tech",
                "china" => "hint_china",
                _ => "hint_military",
            });
        }
    }
    for (key, yes) in [
        ("cn_equity", equity != usize::MAX),
        ("first_foreign", foreign < domestic && foreign != usize::MAX),
        (
            "first_domestic",
            domestic < foreign && domestic != usize::MAX,
        ),
        ("mixed", pos[0] != usize::MAX && pos[2] != usize::MAX),
        (
            "local_foreign_brand",
            pos[1] != usize::MAX && pos[4] != usize::MAX,
        ),
        (
            "foreign_report_domestic",
            pos[5] != usize::MAX && pos[2] != usize::MAX,
        ),
        ("legal_event", legal),
        ("financial_event", finance),
        ("fiction", fiction),
        ("fiction_law", fiction && legal),
        ("fiction_finance", fiction && finance),
        (
            "clinical",
            terms("clinical_terms").any(|s| text.contains(&s.to_lowercase())),
        ),
        (
            "education",
            any(
                text,
                &[
                    "招生", "高考", "中考", "录取", "学费", "教学", "课程", "助学", "入学",
                ],
            ),
        ),
        (
            "military",
            any(
                text,
                &[
                    "军演", "实弹", "部队", "战机", "导弹", "航母", "军队", "国防",
                ],
            ),
        ),
        (
            "government",
            any(
                text,
                &[
                    "国务院",
                    "全国人大",
                    "全国政协",
                    "党委",
                    "中央纪委",
                    "党籍",
                    "省委",
                    "政务",
                ],
            ),
        ),
        (
            "diplomatic_event",
            any(
                text,
                &[
                    "外长",
                    "外交部",
                    "外交会谈",
                    "中方代表",
                    "联合国",
                    "国事访问",
                ],
            ) && any(text, &["会谈", "通话", "访问", "谈判", "峰会", "双边关系"]),
        ),
    ] {
        if yes {
            out.push(key);
        }
    }
    out
}
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn features(role: &str, text: &str) -> HashMap<u64, u16> {
    let tokens = units(text);
    let mut out = HashMap::<u64, u16>::new();
    for n in 1..=4.min(tokens.len()) {
        for gram in tokens.windows(n) {
            let count = out
                .entry(hash(&format!("{role}|g:{}", gram.join("\u{1f}"))))
                .or_default();
            *count = count.saturating_add(1);
        }
    }
    for flag in flags(text) {
        *out.entry(hash(&format!("{role}|k:{flag}"))).or_default() += 1;
    }
    out
}
struct Model {
    data: &'static [u8],
    offset: usize,
    count: usize,
    scale: f32,
    classes: Vec<&'static str>,
    bias: Vec<f32>,
    weights: [f32; 3],
}
fn float(data: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(data[i..i + 4].try_into().unwrap())
}
impl Model {
    fn load(data: &'static [u8]) -> Self {
        assert_eq!(&data[..4], b"PNC2");
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let mut offset = 28;
        let mut classes = vec![];
        let mut bias = vec![];
        for _ in 0..size {
            let length = data[offset] as usize;
            offset += 1;
            classes.push(std::str::from_utf8(&data[offset..offset + length]).unwrap());
            offset += length;
            bias.push(float(data, offset));
            offset += 4;
        }
        assert_eq!(data.len(), offset + count * (12 + size * 2));
        Self {
            data,
            offset,
            count,
            classes,
            bias,
            scale: float(data, 12),
            weights: [float(data, 16), float(data, 20), float(data, 24)],
        }
    }
    fn predict(&self, views: &[HashMap<u64, u16>; 3]) -> (&'static str, f32, f32) {
        let mut scores = self.bias.clone();
        let stride = 12 + scores.len() * 2;
        for (view, counts) in views.iter().enumerate() {
            let mut totals = vec![0.0; scores.len()];
            let mut length = 0.0_f32;
            for (&key, &count) in counts {
                let mut lo = 0;
                let mut hi = self.count;
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let offset = self.offset + mid * stride;
                    let found =
                        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap());
                    match found.cmp(&key) {
                        std::cmp::Ordering::Less => lo = mid + 1,
                        std::cmp::Ordering::Greater => hi = mid,
                        std::cmp::Ordering::Equal => {
                            let weight =
                                (1.0 + f32::from(count).ln()) * float(self.data, offset + 8);
                            length += weight * weight;
                            for (i, total) in totals.iter_mut().enumerate() {
                                let start = offset + 12 + i * 2;
                                *total += weight
                                    * f32::from(i16::from_le_bytes(
                                        self.data[start..start + 2].try_into().unwrap(),
                                    ))
                                    * self.scale;
                            }
                            break;
                        }
                    }
                }
            }
            let norm = length.sqrt().max(f32::EPSILON);
            for (score, value) in scores.iter_mut().zip(totals) {
                *score += value / norm * self.weights[view];
            }
        }
        let mut order: Vec<_> = (0..scores.len()).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
        let foreign_gap = self
            .classes
            .iter()
            .position(|&s| s == "foreign")
            .map(|i| {
                scores
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, &s)| s)
                    .fold(f32::NEG_INFINITY, f32::max)
                    - scores[i]
            })
            .unwrap_or(0.0);
        (
            self.classes[order[0]],
            scores[order[0]] - scores[order[1]],
            foreign_gap,
        )
    }
}
pub struct Prediction {
    pub category: &'static str,
    pub margin: f32,
    pub scope: &'static str,
    pub scope_margin: f32,
    pub topic: &'static str,
    pub topic_margin: f32,
    pub reliable: bool,
}
pub fn predict(title: &str, body: &str) -> Prediction {
    static MODELS: OnceLock<(Model, Model)> = OnceLock::new();
    let (topic_model, scope_model) =
        MODELS.get_or_init(|| (Model::load(TOPIC), Model::load(SCOPE)));
    let title: String = clean(title).chars().take(240).collect();
    let body: String = clean(body).chars().take(1600).collect();
    let lead: String = body.chars().take(320).collect();
    let views = [
        features("t", &title),
        features("l", &lead),
        features("b", &body),
    ];
    let (topic, topic_margin, _) = topic_model.predict(&views);
    let (scope, scope_margin, foreign_gap) = scope_model.predict(&views);
    let foreign = scope == "foreign";
    let margin = if foreign {
        scope_margin
    } else {
        topic_margin.min(foreign_gap)
    };
    Prediction {
        category: if foreign { "world" } else { topic },
        margin,
        scope,
        scope_margin,
        topic,
        topic_margin,
        reliable: margin >= 0.45,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantized_models_match_training_predictions() {
        let rows: serde_json::Value =
            serde_json::from_str(include_str!("../training/classifier-parity.json")).unwrap();
        for row in rows.as_array().unwrap() {
            let p = predict(
                row["title"].as_str().unwrap(),
                row["body"].as_str().unwrap(),
            );
            for (name, category, margin) in [
                ("scope", p.scope, p.scope_margin),
                ("topic", p.topic, p.topic_margin),
            ] {
                assert_eq!(
                    category,
                    row[name]["category"].as_str().unwrap(),
                    "{}: {}",
                    name,
                    row["title"]
                );
                assert!(
                    (margin - row[name]["margin"].as_f64().unwrap() as f32).abs() < 0.005,
                    "{name}: {margin} vs {}",
                    row[name]["margin"]
                );
            }
        }
        assert!(!flags("chair metadata faad").contains(&"foreign_actors"));
        assert_eq!(units(&clean("ＡＩ２０２６ USB-C")), ["@ai#", "@usb", "@c"]);
    }
}

// Geographic anchors constrain decoding; a publisher or background brand is not a subject.
pub(crate) fn scope_anchor(title: &str, body: &str) -> Option<&'static str> {
    let title = clean(title);
    let first = |key: &str| {
        terms(key)
            .filter_map(|s| position(&title, s))
            .min()
            .unwrap_or(usize::MAX)
    };
    let foreign_place = first("foreign_places");
    let diplomatic = foreign_place != usize::MAX
        && any(
            &title,
            &["外长", "部长", "政府", "主席", "总统", "代表", "监管"],
        )
        && any(
            &title,
            &[
                "会谈",
                "通电话",
                "会晤",
                "共同主持",
                "合作会议",
                "双边",
                "签署",
            ],
        );
    if diplomatic {
        return Some("foreign");
    }
    if flags(&title).contains(&"cn_equity") || first("domestic_context") != usize::MAX {
        return Some("domestic");
    }
    let domestic = first("domestic_actors").min(first("domestic_places"));
    let mut foreign_actor = first("foreign_actors");
    if title.starts_with("苹果")
        && any(
            &title,
            &["手机", "iphone", "财报", "净利润", "营收", "ceo", "芯片"],
        )
    {
        foreign_actor = 0;
    }
    for action in ["改造为", "适配", "支持", "兼容", "用于"] {
        if title.find(action).is_some_and(|i| i < foreign_actor) {
            foreign_actor = usize::MAX;
        }
    }
    let foreign = foreign_place.min(foreign_actor);
    if foreign < domestic {
        return Some("foreign");
    }
    if domestic < usize::MAX {
        return Some("domestic");
    }
    // A foreign-media headline can use its lead to identify the main event's location.
    if first("foreign_reports") != usize::MAX {
        let lead: String = clean(body).chars().take(320).collect();
        if terms("foreign_places")
            .chain(terms("foreign_actors"))
            .any(|s| position(&lead, s).is_some())
        {
            return Some("foreign");
        }
    }
    None
}
