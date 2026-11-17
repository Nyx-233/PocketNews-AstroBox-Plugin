use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

pub const MAX_ARTICLES: usize = 100;
pub const DEFAULT_ARTICLES: usize = 30;
pub const CATEGORIES: [(&str, &str); 13] = [
    ("hot", "热点"),
    ("china", "国内"),
    ("world", "国际"),
    ("tech", "科技"),
    ("finance", "财经"),
    ("sports", "体育"),
    ("entertainment", "娱乐"),
    ("military", "军事"),
    ("society", "社会"),
    ("culture", "文化"),
    ("education", "教育"),
    ("health", "健康"),
    ("life", "生活"),
];
pub fn default_article_count() -> usize {
    DEFAULT_ARTICLES
}

// Every user setting persists in cache/news.json; fetched news stays in memory only.
#[derive(Serialize, Deserialize)]
pub struct Saved {
    #[serde(default = "default_article_count")]
    pub article_count: usize,
    // Channels and sources start empty: the user picks them before the first fetch.
    #[serde(default)]
    pub preferences: Vec<String>,
    #[serde(default)]
    pub source_preferences: Vec<String>,
    #[serde(default = "default_open")]
    pub settings_open: bool,
    #[serde(default)]
    pub articles_open: bool,
    #[serde(skip)]
    pub articles: Vec<Article>,
    #[serde(skip)]
    pub fetched_at: u64,
}

fn default_open() -> bool {
    true
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            article_count: DEFAULT_ARTICLES,
            preferences: vec![],
            source_preferences: vec![],
            settings_open: true,
            articles_open: false,
            articles: vec![],
            fetched_at: 0,
        }
    }
}

// Host input events may carry a DOM value or a slider value array.
pub fn article_count(payload: &str) -> Option<usize> {
    fn number(value: &serde_json::Value) -> Option<f64> {
        match value {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.trim().parse().ok(),
            serde_json::Value::Array(a) => a.first().and_then(number),
            serde_json::Value::Object(o) => o
                .get("value")
                .or_else(|| o.get("target"))
                .or_else(|| o.get("detail"))
                .and_then(number),
            _ => None,
        }
    }
    let value = serde_json::from_str(payload)
        .ok()
        .and_then(|v| number(&v))
        .or_else(|| payload.trim().parse::<f64>().ok())?;
    value
        .is_finite()
        .then(|| value.round().clamp(1.0, MAX_ARTICLES as f64) as usize)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub source: String,
    pub url: String,
    pub category: String,
    // Popularity is orthogonal to the article's subject; it is only used by phone-side filters.
    #[serde(skip)]
    pub hot: bool,
    pub published_at: Option<u64>,
}
impl Article {
    pub fn matches_category(&self, category: &str) -> bool {
        self.category == category || (category == "hot" && self.hot)
    }
}

// Resolve a redirect or relative link to an HTTPS address without a fragment.
pub fn resolve_url(base: &str, value: &str) -> Option<String> {
    let mut parsed = url::Url::parse(base).ok()?.join(value.trim()).ok()?;
    if parsed.scheme() == "http" {
        parsed.set_scheme("https").ok()?;
    }
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return None;
    }
    parsed.set_fragment(None);
    Some(parsed.to_string())
}
fn canonical_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return value.to_owned();
    };
    url.set_fragment(None);
    if url.scheme() == "http" {
        let _ = url.set_scheme("https");
    }
    if let Some(host) = url
        .host_str()
        .and_then(|h| h.strip_prefix("www."))
        .map(str::to_owned)
    {
        let _ = url.set_host(Some(&host));
    }
    let mut query: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| {
            !key.starts_with("utm_")
                && !["spm", "from", "ref", "refer", "share_source", "share_token"]
                    .contains(&key.as_ref())
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    query.sort();
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    url.to_string()
}
struct Headline {
    text: String,
    chars: Vec<char>,
    numbers: Vec<String>,
    polarity: Vec<&'static str>,
}
impl Headline {
    fn new(title: &str) -> Self {
        let mut title = title.trim();
        for prefix in [
            "【快讯】",
            "【突发】",
            "【独家】",
            "【最新】",
            "快讯：",
            "快讯:",
            "突发：",
            "最新消息：",
        ] {
            if let Some(rest) = title.strip_prefix(prefix) {
                title = rest.trim();
            }
        }
        let folded: String = title
            .chars()
            .map(|c| {
                if ('！'..='～').contains(&c) {
                    char::from_u32(c as u32 - 0xfee0).unwrap()
                } else {
                    c
                }
            })
            .collect::<String>()
            .to_lowercase();
        let text: String = folded.chars().filter(|c| c.is_alphanumeric()).collect();
        let numbers = folded
            .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
            .filter(|s| s.chars().any(|c| c.is_ascii_digit()))
            .map(str::to_owned)
            .collect();
        let polarity = [
            "不", "未", "无", "否认", "辟谣", "上涨", "下跌", "增加", "减少", "上调", "下调",
        ]
        .into_iter()
        .filter(|word| text.contains(word))
        .collect();
        Self {
            chars: text.chars().collect(),
            text,
            numbers,
            polarity,
        }
    }
    fn same_story(&self, other: &Self) -> bool {
        // Short/generic titles do not identify an event. Preserve numbers and opposing claims.
        if self.chars.len().min(other.chars.len()) < 10
            || self.numbers != other.numbers
            || self.polarity != other.polarity
        {
            return false;
        }
        if self.text == other.text {
            return true;
        }
        let (a, b) = (&self.chars, &other.chars);
        if a.len().min(b.len()) < 20 {
            return false;
        }
        let budget = (a.len().min(b.len()) / 12).min(6);
        if a.len().abs_diff(b.len()) > budget {
            return false;
        }
        // Require the same event's opening subject. A changed country, company or person at
        // the beginning must not be merged just because the rest is a stock headline template.
        if a[..6] != b[..6] {
            return false;
        }
        // Only added/removed wording is accepted. Substituting a name/location is a new story.
        let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        let mut matched = 0;
        for c in long {
            if short.get(matched) == Some(c) {
                matched += 1;
            }
        }
        matched == short.len()
    }
}

fn body_quality(article: &Article) -> usize {
    if article
        .body
        .contains("该来源仅提供标题，请在手机打开原文阅读。")
    {
        0
    } else {
        article.body.chars().count()
    }
}

// Merge before filtering and counting. Retain the complete article (including its own source
// and original URL), and preserve trending membership from every duplicate copy.
pub fn deduplicate(articles: Vec<Article>) -> Vec<Article> {
    let mut out: Vec<Article> = vec![];
    let mut headlines: Vec<Vec<Headline>> = vec![];
    let mut identities = HashMap::new();
    for article in articles {
        let url = canonical_url(&article.url);
        let title = Headline::new(&article.title);
        let body_key = if body_quality(&article) >= 120 {
            use sha1::Digest;
            let body = crate::classification::content_text(&article.body);
            let normalized: String = body.chars().filter(|c| c.is_alphanumeric()).collect();
            Some(format!(
                "body:{:x}",
                sha1::Sha1::digest(normalized.as_bytes())
            ))
        } else {
            None
        };
        let duplicate = identities
            .get(&format!("id:{}", article.id))
            .copied()
            .or_else(|| identities.get(&format!("url:{url}")).copied())
            .or_else(|| {
                body_key
                    .as_ref()
                    .and_then(|key| identities.get(key).copied())
            })
            .or_else(|| {
                headlines
                    .iter()
                    .enumerate()
                    .find(|(i, variants)| {
                        let recent = match (out[*i].published_at, article.published_at) {
                            (Some(a), Some(b)) => a.abs_diff(b) <= 24 * 60 * 60 * 1000,
                            _ => true,
                        };
                        recent && variants.iter().any(|other| title.same_story(other))
                    })
                    .map(|(i, _)| i)
            });
        let index = duplicate.unwrap_or(out.len());
        identities.insert(format!("id:{}", article.id), index);
        identities.insert(format!("url:{url}"), index);
        if let Some(key) = body_key {
            identities.insert(key, index);
        }
        if duplicate.is_some() {
            let existing = &mut out[index];
            let hot = existing.hot || article.hot;
            let published = article.published_at.or(existing.published_at);
            if (
                body_quality(&article),
                article.summary.chars().count(),
                article.published_at,
            ) > (
                body_quality(existing),
                existing.summary.chars().count(),
                existing.published_at,
            ) {
                *existing = article;
            }
            existing.hot = hot;
            if existing.published_at.is_none() {
                existing.published_at = published;
            }
            if !headlines[index].iter().any(|h| h.text == title.text) {
                headlines[index].push(title);
            }
        } else {
            out.push(article);
            headlines.push(vec![title]);
        }
    }
    out
}

// Keep recent items within each category, then interleave categories without padding a short feed.
pub fn collect_batch(articles: Vec<Article>, categories: &[String]) -> Vec<Article> {
    collect_batch_limit(articles, categories, DEFAULT_ARTICLES)
}
pub fn collect_batch_limit(
    mut articles: Vec<Article>,
    categories: &[String],
    count: usize,
) -> Vec<Article> {
    let count = count.clamp(1, MAX_ARTICLES);
    articles.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    let mut articles = deduplicate(articles);
    articles.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    let articles: Vec<_> = articles
        .into_iter()
        .filter(|a| categories.iter().any(|c| a.matches_category(c)))
        .collect();
    let mut seen_categories = HashSet::new();
    let mut groups: Vec<VecDeque<Article>> = categories
        .iter()
        .filter(|key| seen_categories.insert(key.as_str()))
        .map(|key| {
            articles
                .iter()
                .filter(|a| a.matches_category(key))
                .cloned()
                .collect()
        })
        .collect();
    let mut chosen = Vec::with_capacity(count);
    let mut chosen_ids = HashSet::new();
    while chosen.len() < count {
        let before = chosen.len();
        for group in &mut groups {
            while let Some(article) = group.pop_front() {
                if chosen_ids.insert(article.id.clone()) {
                    chosen.push(article);
                    break;
                }
            }
            if chosen.len() == count {
                break;
            }
        }
        if chosen.len() == before {
            break;
        }
    }
    chosen
}

pub fn truncate(s: &str, count: usize) -> String {
    s.chars().take(count).collect()
}
pub fn publication_date(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp as i64)
        .map(|date| {
            (date + chrono::Duration::hours(8))
                .format("%Y/%m/%d")
                .to_string()
        })
        .unwrap_or_default()
}
pub fn hash(s: &str) -> String {
    format!(
        "{:08x}",
        s.bytes().fold(0x811c9dc5u32, |h, b| (h ^ b as u32)
            .wrapping_mul(0x01000193))
    )
}
pub fn plain(s: &str) -> String {
    let s = s
        .replace("</p>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    let mut inside = false;
    let stripped: String = s
        .chars()
        .filter(|c| {
            if *c == '<' {
                inside = true;
                false
            } else if *c == '>' {
                inside = false;
                false
            } else {
                !inside
            }
        })
        .collect();
    let decoded = stripped
        .replace("&nbsp;", " ")
        .replace("&ldquo;", "“")
        .replace("&rdquo;", "”")
        .replace("&mdash;", "—");
    let decoded = quick_xml::escape::unescape(&decoded)
        .map(|x| x.into_owned())
        .unwrap_or(decoded);
    // Wearable fonts do not support emoji or supplementary-plane symbols.
    decoded
        .chars()
        .filter(|c| {
            (*c as u32) < 0x10000
                && !matches!(*c as u32, 0x2600..=0x27ff | 0xfe00..=0xfe0f | 0x200d | 0x20e3)
        })
        .collect::<String>()
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub(crate) fn article(
    title: &str,
    body: &str,
    url: &str,
    source: &str,
    evidence: crate::classification::Evidence<'_>,
    published_at: Option<u64>,
) -> Option<Article> {
    let title = truncate(&plain(title), 180);
    let url = url.trim().replace("http://", "https://");
    if title.is_empty() || !url.starts_with("https://") || url.len() > 1500 {
        return None;
    }
    // The wearable is text-only: pictures in the source markup are dropped with the other tags.
    let body = plain(body);
    let category = crate::classification::classify(
        &title,
        &body,
        &url,
        evidence.channels,
        evidence.tags,
        evidence.fallback,
    )
    .into();
    let summary = truncate(&body.replace('\n', " "), 120);
    Some(Article {
        id: hash(&url),
        title,
        summary,
        body,
        source: source.into(),
        url,
        category,
        hot: false,
        published_at,
    })
}
pub fn parse_rss(xml: &str, source: &str, category: &str) -> Result<Vec<Article>, String> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    let (mut active, mut field) = (false, String::new());
    let mut item_channels = Vec::new();
    let mut channel_name = String::new();
    let (mut title, mut body, mut content, mut link, mut date, mut updated) = (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    );
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("RSS 解析失败: {e}"))?
        {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "item" || name == "entry" {
                    active = true;
                    title.clear();
                    body.clear();
                    content.clear();
                    link.clear();
                    date.clear();
                    updated.clear();
                    item_channels.clear();
                    channel_name.clear();
                }
                if active {
                    if name == "category" {
                        channel_name.clear();
                        for attribute in e.attributes().flatten() {
                            if matches!(attribute.key.as_ref(), b"term" | b"label") {
                                if let Ok(value) =
                                    attribute.decode_and_unescape_value(reader.decoder())
                                {
                                    item_channels.push(value.into_owned());
                                }
                            }
                        }
                    }
                    field = name;
                }
            }
            Event::Text(e) if active => {
                let text = e.decode().map_err(|e| e.to_string())?;
                match field.as_str() {
                    "title" => title.push_str(&text),
                    "description" | "summary" => body.push_str(&text),
                    "content:encoded" | "content" => content.push_str(&text),
                    "link" => link.push_str(&text),
                    "category" => channel_name.push_str(&text),
                    "pubDate" | "published" => date.push_str(&text),
                    "updated" => updated.push_str(&text),
                    _ => {}
                }
            }
            Event::CData(e) if active => {
                let text = e.decode().map_err(|e| e.to_string())?;
                match field.as_str() {
                    "title" => title.push_str(&text),
                    "description" | "summary" => body.push_str(&text),
                    "content:encoded" | "content" => content.push_str(&text),
                    "link" => link.push_str(&text),
                    "category" => channel_name.push_str(&text),
                    "pubDate" | "published" => date.push_str(&text),
                    "updated" => updated.push_str(&text),
                    _ => {}
                }
            }
            Event::GeneralRef(e) if active => {
                let reference = format!("&{};", String::from_utf8_lossy(e.as_ref()));
                let text = quick_xml::escape::unescape(&reference)
                    .map(|s| s.into_owned())
                    .unwrap_or(reference);
                match field.as_str() {
                    "title" => title.push_str(&text),
                    "description" | "summary" => body.push_str(&text),
                    "content:encoded" | "content" => content.push_str(&text),
                    "link" => link.push_str(&text),
                    "category" => channel_name.push_str(&text),
                    _ => {}
                }
            }
            Event::Empty(e) if active && e.name().as_ref() == b"link" => {
                let mut href = None;
                let mut alternate = true;
                for attribute in e.attributes().flatten() {
                    if attribute.key.as_ref() == b"href" {
                        href = Some(String::from_utf8_lossy(&attribute.value).into_owned());
                    }
                    if attribute.key.as_ref() == b"rel" && attribute.value.as_ref() != b"alternate"
                    {
                        alternate = false;
                    }
                }
                if alternate {
                    if let Some(href) = href {
                        link = href;
                    }
                }
            }
            Event::Empty(e) if active && e.name().as_ref() == b"category" => {
                for attribute in e.attributes().flatten() {
                    if matches!(attribute.key.as_ref(), b"term" | b"label") {
                        if let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) {
                            item_channels.push(value.into_owned());
                        }
                    }
                }
            }
            Event::End(e) if active && e.name().as_ref() == b"category" => {
                item_channels.push(std::mem::take(&mut channel_name));
                field.clear();
            }
            Event::End(e) if e.name().as_ref() == b"item" || e.name().as_ref() == b"entry" => {
                let timestamp = crate::article::date(if date.trim().is_empty() {
                    updated.trim()
                } else {
                    date.trim()
                });
                if let Some(a) = article(
                    &title,
                    if content.is_empty() { &body } else { &content },
                    &link,
                    source,
                    crate::classification::Evidence {
                        channels: &item_channels,
                        fallback: category,
                        ..Default::default()
                    },
                    timestamp,
                ) {
                    result.push(a);
                }
                active = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if result.is_empty() {
        return Err("该来源没有可用新闻".into());
    }
    Ok(result)
}

// Adapted from ourongxing/newsnow (MIT): wallstreetcn.ts.
pub fn parse_json(text: &str, source: &str, _category: &str) -> Result<Vec<Article>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let items = value.pointer("/data/items");
    let items = items
        .and_then(|v| v.as_array())
        .ok_or("新闻接口数据格式已变化")?;
    let mut out = Vec::new();
    for item in items {
        let (title, body, url, date) = {
            let body = item["content"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| item["content_text"].as_str())
                .unwrap_or("");
            (
                item["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(body)
                    .to_owned(),
                body.to_owned(),
                item["uri"].as_str().unwrap_or("").to_owned(),
                item["display_time"].as_u64().map(|t| t * 1000),
            )
        };
        let explicit = crate::classification::json_evidence(
            item,
            "/category_name|/channel_name|/category|/global_channel_name",
        );
        let tags = crate::classification::json_evidence(item, "/tags|/related_themes");
        if let Some(mut a) = article(
            &title,
            &body,
            &url,
            source,
            crate::classification::Evidence {
                channels: &explicit,
                tags: &tags,
                ..Default::default()
            },
            date,
        ) {
            a.hot = source != "华尔街见闻";
            out.push(a);
        }
    }
    if out.is_empty() {
        return Err("该来源没有可用新闻".into());
    }
    Ok(out)
}
