// Source definitions and field mappings adapted from NewsNow (MIT).
// Fetch publishers directly; keep the watch's existing thirteen channel IDs.
use crate::{
    classification,
    news::{self, Article},
};
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Feed {
    pub id: String,
    pub provider: String,
    pub category: String,
    pub mixed: bool,
    pub channel: String,
    pub tags: String,
    pub hot: bool,
    pub url: String,
    pub kind: String,
    pub items: String,
    pub title: String,
    pub link: String,
    pub template: String,
    pub body: String,
    pub date: String,
    pub seconds: bool,
    pub referer: String,
    pub encoding: String,
    pub cookie_url: String,
    pub extract: String,
    pub post: Option<Value>,
    pub row: String,
    pub anchor: String,
    pub title_selector: String,
    pub description_selector: String,
    pub link_selector: String,
    pub link_attr: String,
    pub channel_selector: String,
    pub date_selector: String,
    pub detail: String,
    pub problem: String,
}
#[derive(Deserialize)]
struct Catalog {
    providers: Vec<Provider>,
    feeds: Vec<Feed>,
}
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("source-catalog.json"))
            .expect("Valid bundled source catalog")
    })
}
pub fn providers() -> &'static [Provider] {
    &catalog().providers
}
pub fn feeds() -> &'static [Feed] {
    &catalog().feeds
}
pub fn normalize_selection(ids: &[String]) -> Vec<String> {
    providers()
        .iter()
        .filter(|p| ids.contains(&p.id))
        .map(|p| p.id.clone())
        .collect()
}
pub fn provider_name(id: &str) -> &str {
    providers()
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.as_str())
        .unwrap_or(id)
}
pub fn jobs(categories: &[String], selected: &[String]) -> Vec<usize> {
    let valid_categories = categories
        .iter()
        .any(|c| news::CATEGORIES.iter().any(|(id, _)| c == id));
    feeds()
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            valid_categories
                && selected.contains(&f.provider)
                && (f.mixed || categories.contains(&f.category))
        })
        .map(|(i, _)| i)
        .collect()
}
pub fn timestamp(value: &str, seconds: bool) -> Option<u64> {
    if value.trim().is_empty() {
        return None;
    }
    if let Ok(n) = value.parse::<u64>() {
        return Some(if seconds { n.saturating_mul(1000) } else { n });
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(value) {
        return u64::try_from(d.timestamp_millis()).ok();
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc2822(value) {
        return u64::try_from(d.timestamp_millis()).ok();
    }
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y年%m月%d日 %H:%M:%S",
        "%Y年%m月%d日 %H:%M",
    ] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return u64::try_from(d.and_utc().timestamp_millis() - 8 * 3600 * 1000).ok();
        }
    }
    for format in ["%Y-%m-%d", "%Y/%m/%d", "%Y年%m月%d日"] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(value.trim(), format) {
            return u64::try_from(
                date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis() - 8 * 3600 * 1000,
            )
            .ok();
        }
    }
    None
}
fn field(value: &Value, paths: &str) -> String {
    paths
        .split('|')
        .find_map(|path| {
            if path.is_empty() {
                return None;
            }
            let v = value.pointer(path)?;
            match v {
                Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
        })
        .unwrap_or_default()
}
fn template(pattern: &str, row: &Value) -> String {
    let mut result = pattern.to_owned();
    while let Some(start) = result.find('{') {
        let Some(end) = result[start..].find('}').map(|i| i + start) else {
            break;
        };
        let value = field(row, &result[start + 1..end]);
        if value.is_empty() {
            return String::new();
        }
        let encoded = url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>();
        result.replace_range(start..=end, &encoded);
    }
    result
}
fn selector(value: &str) -> Result<Selector, String> {
    Selector::parse(value).map_err(|_| "来源页面选择器无效".into())
}
fn find<'a>(element: ElementRef<'a>, query: &str) -> Option<ElementRef<'a>> {
    if query.is_empty() {
        Some(element)
    } else {
        element.select(&Selector::parse(query).ok()?).next()
    }
}
fn text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn absolute(base: &str, link: &str) -> Option<String> {
    let base = url::Url::parse(base).ok()?;
    let mut result = base.join(link).ok()?;
    if result.scheme() == "http" {
        result.set_scheme("https").ok()?;
    }
    (result.scheme() == "https").then(|| result.to_string())
}
fn parse_html(feed: &Feed, input: &str, category: &str) -> Result<Vec<Article>, String> {
    let document = Html::parse_document(input);
    let mut out = vec![];
    for row in document.select(&selector(&feed.row)?).take(100) {
        let Some(anchor) = find(row, &feed.anchor) else {
            continue;
        };
        let title = find(anchor, &feed.title_selector)
            .map(text)
            .unwrap_or_default();
        let link_element = if feed.link_selector.is_empty() {
            anchor
        } else {
            let Some(e) = find(row, &feed.link_selector) else {
                continue;
            };
            e
        };
        let mut link = if feed.link_attr.is_empty() {
            link_element.attr("href").unwrap_or("").to_owned()
        } else {
            feed.link_attr
                .split('|')
                .find_map(|key| link_element.attr(key))
                .unwrap_or("")
                .to_owned()
        };
        if !feed.template.is_empty() && !link.starts_with('/') && !link.starts_with("http") {
            link = feed.template.replace("{id}", &link);
        }
        if link.is_empty() {
            continue;
        }
        let Some(link) = absolute(&feed.url, &link) else {
            continue;
        };
        let body = if feed.description_selector.is_empty() {
            String::new()
        } else {
            find(row, &feed.description_selector)
                .map(|e| e.inner_html())
                .unwrap_or_default()
        };
        let published = find(row, "[data-unixtime]")
            .and_then(|e| e.attr("data-unixtime"))
            .and_then(|s| timestamp(s, true))
            .or_else(|| {
                if feed.date_selector.is_empty() {
                    return None;
                }
                let element = find(row, &feed.date_selector)?;
                crate::article::date(element.attr("datetime").unwrap_or(&text(element)))
            });
        let explicit = if feed.channel_selector.is_empty() {
            vec![]
        } else {
            row.select(&selector(&feed.channel_selector)?)
                .map(|element| {
                    element
                        .attr("data-channel")
                        .map(str::to_owned)
                        .unwrap_or_else(|| text(element))
                })
                .chain(row.attr("data-channel").map(str::to_owned))
                .collect::<Vec<_>>()
        };
        if let Some(a) = news::article(
            &title,
            &body,
            &link,
            provider_name(&feed.provider),
            classification::Evidence {
                channels: &explicit,
                fallback: category,
                ..Default::default()
            },
            published,
        ) {
            out.push(a);
        }
    }
    Ok(out)
}
fn embedded(input: &str, marker: &str) -> Result<Value, String> {
    for (position, _) in input.match_indices(marker) {
        let mut remaining = input[position + marker.len()..].trim_start();
        if remaining.starts_with('=') && !remaining.starts_with("==") {
            remaining = remaining[1..].trim_start();
        }
        if !remaining.starts_with(['{', '[']) {
            continue;
        }
        if let Ok(value) = Value::deserialize(&mut serde_json::Deserializer::from_str(remaining)) {
            return Ok(value);
        }
    }
    Err("来源页面缺少可读取的 JSON 数据".into())
}
pub fn decode_text(bytes: &[u8], preferred: &str) -> Result<String, String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_lowercase();
    let gbk = preferred == "gbk"
        || [
            "charset=gb2312",
            "charset=gbk",
            "charset=\"gb2312",
            "charset=\"gbk",
            "encoding=\"gb2312",
        ]
        .iter()
        .any(|s| head.contains(s));
    let text = if gbk {
        let (text, _, error) = encoding_rs::GBK.decode(bytes);
        if error {
            return Err("来源 GBK 编码无效".into());
        }
        text.into_owned()
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|_| "来源编码不是 UTF-8".to_owned())?
    };
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}
pub fn parse_response(index: usize, input: &str) -> Result<Vec<Article>, String> {
    let feed = feeds().get(index).ok_or("无效新闻来源")?;
    let source = provider_name(&feed.provider);
    let category = if feed.mixed { "" } else { &feed.category };
    let mut result = match feed.kind.as_str() {
        "legacy" => {
            if input.trim_start().starts_with('{') {
                news::parse_json(input, source, category)?
            } else {
                news::parse_rss(input, source, category)?
            }
        }
        "rss" => news::parse_rss(input, source, category)?,
        "html" => parse_html(feed, input, category)?,
        "json" => {
            let value = if !feed.extract.is_empty() {
                embedded(input, &feed.extract)?
            } else {
                serde_json::from_str(input).map_err(|e| format!("来源数据解析失败: {e}"))?
            };
            let items_value = value.pointer(&feed.items).ok_or("新闻接口数据格式已变化")?;
            let decoded_items;
            let items = if let Some(text) = items_value.as_str() {
                decoded_items =
                    serde_json::from_str::<Value>(text).map_err(|_| "来源内嵌列表格式已变化")?;
                decoded_items.as_array()
            } else {
                items_value.as_array()
            }
            .ok_or("新闻接口数据格式已变化")?;
            let mut out = vec![];
            for row in items.iter().take(100) {
                if row["isTop"] == true || row["is_ad"] == 1 || row["ad"] == 1 {
                    continue;
                }
                if feed.provider == "jin10"
                    && row["channel"]
                        .as_array()
                        .is_some_and(|a| a.contains(&Value::from(5)))
                {
                    continue;
                }
                let title = field(row, &feed.title);
                let mut link = field(row, &feed.link);
                if link.is_empty() && !feed.template.is_empty() {
                    link = template(&feed.template, row);
                }
                if link.is_empty() {
                    continue;
                }
                let link = quick_xml::escape::unescape(&link)
                    .map(|s| s.into_owned())
                    .unwrap_or(link);
                let Some(link) = news::resolve_url(&feed.url, &link) else {
                    continue;
                };
                let body = field(row, &feed.body);
                let date = timestamp(&field(row, &feed.date), feed.seconds);
                let explicit = classification::json_evidence(row, &feed.channel);
                let tags = classification::json_evidence(row, &feed.tags);
                if let Some(a) = news::article(
                    &title,
                    &body,
                    &link,
                    source,
                    classification::Evidence {
                        channels: &explicit,
                        tags: &tags,
                        fallback: category,
                    },
                    date,
                ) {
                    out.push(a);
                }
            }
            out
        }
        _ => return Err("未知来源解析方式".into()),
    };
    for article in &mut result {
        article.hot |= feed.hot || feed.category == "hot";
    }
    let mut seen = std::collections::HashSet::new();
    result.retain(|a| seen.insert(a.id.clone()));
    if result.is_empty() {
        return Err("该来源没有可用新闻，可能需要稍后重试".into());
    }
    Ok(result)
}

// Admit only dated, self-contained text. Short genuine flashes are valid; an arbitrarily long
// excerpt is not. Quality is determined by the audited feed and explicit truncation markers.
fn content_problem(article: &Article) -> Option<&'static str> {
    if !article.published_at.is_some_and(|time| time > 0) {
        return Some("原文未提供可解析的发布时间（已跳过）");
    }
    let body = article.body.trim();
    if body.is_empty() {
        return Some("原文没有可读取的文字正文（已跳过）");
    }
    if [
        "该来源仅提供标题",
        "请在手机打开原文",
        "登录后阅读",
        "付费后阅读",
        "订阅后阅读",
        "展开全文",
        "点击阅读全文",
        "点击查看全文",
        "查看原文了解更多",
    ]
    .iter()
    .any(|s| body.contains(s))
        || body.ends_with("...")
        || body.ends_with('…')
    {
        return Some("原文只返回摘要、截断文字或要求阅读全文，尚未取得完整正文（已跳过）");
    }
    None
}
pub fn unreadable_reason(article: &Article) -> Option<&'static str> {
    if let Some(reason) = content_problem(article) {
        return Some(reason);
    }
    if !classification::has_reliable_subject(
        &article.title,
        &article.body,
        &article.url,
        &article.category,
    ) {
        return Some("正文和日期已取得，但分类依据不足（已跳过）");
    }
    None
}
pub fn readable_articles(articles: Vec<Article>) -> Vec<Article> {
    articles
        .into_iter()
        .filter(|article| unreadable_reason(article).is_none())
        .collect()
}

#[cfg(target_arch = "wasm32")]
struct Collector {
    index: usize,
    request: crate::http::Request,
    bootstrap: bool,
    headers: Vec<(String, String)>,
    endpoint: String,
    body: Option<String>,
    started: std::time::Instant,
    stage: Stage,
    pending: std::collections::VecDeque<Article>,
    accepted: Vec<Article>,
    issues: Vec<String>,
    detail_target: usize,
    detail_limit: usize,
    detail_attempts: usize,
    complete_entries: usize,
    categories: Vec<String>,
    blocked_responses: u8,
}
#[cfg(target_arch = "wasm32")]
enum Stage {
    List,
    Detail(Article, u8),
    Finished,
}
#[cfg(target_arch = "wasm32")]
impl Collector {
    fn issue_summary(&self) -> String {
        let mut distinct = vec![];
        for issue in &self.issues {
            if !distinct.contains(&issue) {
                distinct.push(issue);
            }
        }
        distinct
            .into_iter()
            .take(3)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("；")
    }
    fn new(index: usize, target: usize, categories: &[String]) -> Result<Self, String> {
        use sha1::Digest;
        let feed = feeds().get(index).ok_or("无效新闻来源")?;
        if !feed.problem.is_empty() {
            return Err(feed.problem.clone());
        }
        let now = crate::app::now();
        let mut endpoint = feed.url.replace("{now}", &now.to_string());
        let mut headers = vec![];
        if !feed.referer.is_empty() {
            headers.push(("referer".into(), feed.referer.clone()));
        }
        if feed.provider == "cls" {
            let query = format!(
                "appName=CailianpressWeb&last_time={}&os=web&refresh_type=1&rn=30&sv=7.7.5",
                now / 1000
            );
            let sha = format!("{:x}", sha1::Sha1::digest(query.as_bytes()));
            let sign = format!("{:x}", md5::Md5::digest(sha.as_bytes()));
            endpoint.push_str(&format!("?{query}&sign={sign}"));
        }
        let body = feed.post.as_ref().map(Value::to_string);
        let bootstrap = !feed.cookie_url.is_empty();
        let request = if bootstrap {
            crate::http::Request::new(&feed.cookie_url, &[], None)?
        } else {
            crate::http::Request::new(&endpoint, &headers, body.as_deref())?
        };
        Ok(Self {
            index,
            request,
            bootstrap,
            headers,
            endpoint,
            body,
            started: std::time::Instant::now(),
            stage: Stage::List,
            pending: std::collections::VecDeque::new(),
            accepted: vec![],
            issues: vec![],
            detail_target: target.max(1),
            detail_limit: (target * 2 + 4).clamp(6, news::MAX_ARTICLES),
            detail_attempts: 0,
            complete_entries: 0,
            categories: categories.to_vec(),
            blocked_responses: 0,
        })
    }

    fn finish(&mut self) -> Option<Result<Vec<Article>, String>> {
        self.stage = Stage::Finished;
        if self.accepted.is_empty() && self.complete_entries == 0 {
            let summary = self.issue_summary();
            Some(Err(if summary.is_empty() {
                "没有同时包含日期、完整正文和明确频道的新闻".into()
            } else {
                summary
            }))
        } else {
            Some(Ok(std::mem::take(&mut self.accepted)))
        }
    }

    fn next_detail(&mut self) -> Option<Result<Vec<Article>, String>> {
        if self.accepted.len() >= self.detail_target || self.detail_attempts >= self.detail_limit {
            return self.finish();
        }
        while let Some(article) = self.pending.pop_front() {
            let parsed = url::Url::parse(&article.url).ok();
            if parsed
                .as_ref()
                .is_some_and(|url| url.host_str() == Some("www.baidu.com") && url.path() == "/s")
            {
                self.issues.push(
                    "榜单提供的是搜索结果地址，没有对应单篇原文，无法补齐正文和发布时间".into(),
                );
                continue;
            }
            self.detail_attempts += 1;
            // This publisher's public site serves HTTP; keep certificate validation enabled
            // for every HTTPS source instead of accepting its mismatched TLS certificate.
            let address = if feeds()[self.index].provider == "dongqiudi" {
                let id = article
                    .url
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(".html");
                if id.chars().all(|c| c.is_ascii_digit()) {
                    format!("https://api.dongqiudi.com/v2/article/detail/{id}")
                } else {
                    article.url.clone()
                }
            } else if article.url.starts_with("https://military.people.com.cn/") {
                article.url.replacen("https://", "http://", 1)
            } else {
                article.url.clone()
            };
            match crate::http::Request::new(
                &address,
                &[(
                    "referer".into(),
                    if feeds()[self.index].provider == "dongqiudi" {
                        "https://n.dongqiudi.com/".into()
                    } else {
                        self.endpoint.clone()
                    },
                )],
                None,
            ) {
                Ok(request) => {
                    self.request = request;
                    self.stage = Stage::Detail(article, 0);
                    return None;
                }
                Err(error) => self.issues.push(error),
            }
        }
        self.finish()
    }

    fn detail_response(
        &mut self,
        mut article: Article,
        depth: u8,
        input: &str,
    ) -> Option<Result<Vec<Article>, String>> {
        if depth == 0 {
            if let Some((address, title)) = crate::article::linked_article(&article.url, input) {
                article.url = address;
                article.id = news::hash(&article.url);
                article.title = title;
                article.published_at = None;
                article.body.clear();
                match crate::http::Request::new(&article.url, &[], None) {
                    Ok(request) => {
                        self.request = request;
                        self.stage = Stage::Detail(article, 1);
                        return None;
                    }
                    Err(error) => {
                        self.issues.push(error);
                        return self.next_detail();
                    }
                }
            }
        }
        match crate::article::parse(&article.url, &article.title, input) {
            Ok(content) => {
                article.body = content.body;
                article.published_at = content.published_at.or(article.published_at);
                article.summary = news::truncate(&article.body.replace('\n', " "), 120);
                if content_problem(&article).is_none() {
                    self.complete_entries += 1;
                }
                article.category = classification::classify(
                    &article.title,
                    &article.body,
                    &article.url,
                    &[],
                    &content.channels,
                    "",
                )
                .into();
                if let Some(reason) = unreadable_reason(&article) {
                    self.issues.push(reason.into());
                } else if self.categories.iter().any(|c| article.matches_category(c)) {
                    self.accepted.push(article);
                } else {
                    self.issues
                        .push("原文已补齐，但该条目的频道不在本次选择中（已跳过）".into());
                }
            }
            Err(error) => {
                let blocked = error.contains("安全验证");
                self.issues.push(error);
                if blocked {
                    return self.finish();
                }
            }
        }
        self.next_detail()
    }

    fn poll(&mut self) -> Option<Result<Vec<Article>, String>> {
        let limit = if matches!(self.stage, Stage::List) {
            25
        } else {
            (25 + self.detail_target as u64 * 2).clamp(30, 75)
        };
        if self.started.elapsed().as_secs() >= limit {
            self.issues
                .push("来源请求或正文补取超时，已保留成功取得的新闻".into());
            return self.finish();
        }
        let response = match self.request.poll()? {
            Ok(response) => response,
            Err(error) => {
                let detail = matches!(self.stage, Stage::Detail(..));
                let blocked = matches!(error.as_str(), "HTTP 401" | "HTTP 403" | "HTTP 567");
                if blocked {
                    self.blocked_responses = self.blocked_responses.saturating_add(1);
                }
                self.issues.push(if blocked {
                    format!("原文服务拒绝匿名访问（{error}），需要站点允许的访问方式")
                } else {
                    error
                });
                return if detail && (!blocked || self.blocked_responses < 2) {
                    self.next_detail()
                } else {
                    self.finish()
                };
            }
        };
        if self.bootstrap {
            let cookie = response
                .cookies
                .iter()
                .filter_map(|s| s.split(';').next())
                .collect::<Vec<_>>()
                .join("; ");
            if !cookie.is_empty() {
                self.headers.push(("cookie".into(), cookie));
            }
            self.request = match crate::http::Request::new(
                &self.endpoint,
                &self.headers,
                self.body.as_deref(),
            ) {
                Ok(request) => request,
                Err(error) => return Some(Err(error)),
            };
            self.bootstrap = false;
            return None;
        }
        let feed = &feeds()[self.index];
        let input = match decode_text(&response.bytes, &feed.encoding) {
            Ok(text) => text,
            Err(error) => return Some(Err(error)),
        };
        let stage = std::mem::replace(&mut self.stage, Stage::Finished);
        if let Stage::Detail(article, depth) = stage {
            return self.detail_response(article, depth, &input);
        }
        match parse_response(self.index, &input) {
            Ok(mut articles) => {
                if let Some(latest) = articles.iter().filter_map(|a| a.published_at).max() {
                    if crate::app::now().saturating_sub(latest) > 30 * 24 * 60 * 60 * 1000 {
                        self.issues.push(format!(
                            "订阅最新条目日期为 {}，源站订阅数据偏旧",
                            news::publication_date(latest)
                        ));
                    }
                }
                if feed.detail == "none" || feed.detail.is_empty() {
                    self.complete_entries = articles
                        .iter()
                        .filter(|a| content_problem(a).is_none())
                        .count();
                    self.accepted = readable_articles(articles);
                    return self.finish();
                }
                // Prefer requested subjects without changing any article's classification.
                // A bounded detail budget must not be spent entirely on unrelated channels.
                articles.sort_by_key(|a| !self.categories.iter().any(|c| a.matches_category(c)));
                for article in articles {
                    if feed.detail == "missing"
                        && article.published_at.is_some()
                        && !article.body.is_empty()
                    {
                        self.accepted.extend(readable_articles(vec![article]));
                    } else {
                        self.pending.push_back(article);
                    }
                }
                self.next_detail()
            }
            Err(error) => {
                self.issues.push(error);
                self.finish()
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Collection {
    queue: std::collections::VecDeque<usize>,
    active: Vec<Collector>,
    pub completed: usize,
    pub failed: usize,
    started: std::time::Instant,
    detail_target: usize,
    warnings: Vec<String>,
    categories: Vec<String>,
}
#[cfg(target_arch = "wasm32")]
impl Collection {
    pub fn new(jobs: Vec<usize>) -> Self {
        Self::with_limit(jobs, news::MAX_ARTICLES)
    }
    pub fn with_limit(jobs: Vec<usize>, count: usize) -> Self {
        Self::with_categories(
            jobs,
            count,
            news::CATEGORIES
                .iter()
                .map(|(id, _)| (*id).to_owned())
                .collect(),
        )
    }
    pub fn with_categories(jobs: Vec<usize>, count: usize, categories: Vec<String>) -> Self {
        let detail_target = count
            .clamp(1, news::MAX_ARTICLES)
            .div_ceil(jobs.len().max(1));
        Self {
            queue: jobs.into(),
            active: vec![],
            completed: 0,
            failed: 0,
            started: std::time::Instant::now(),
            detail_target,
            warnings: vec![],
            categories,
        }
    }
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
    pub fn is_done(&self) -> bool {
        self.queue.is_empty() && self.active.is_empty()
    }
    pub fn elapsed_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
    pub fn active_summary(&self) -> String {
        let mut names = vec![];
        for collector in &self.active {
            let name = provider_name(&feeds()[collector.index].provider);
            let label = if matches!(collector.stage, Stage::Detail(..)) {
                format!(
                    "{name}（补正文 {}/{}）",
                    collector.accepted.len(),
                    collector.detail_target
                )
            } else {
                name.to_owned()
            };
            if !names.contains(&label) {
                names.push(label);
            }
        }
        names.join("、")
    }
    pub fn poll(&mut self) -> Vec<(usize, Result<Vec<Article>, String>)> {
        let mut results = vec![];
        let mut position = 0;
        while position < self.active.len() {
            if let Some(result) = self.active[position].poll() {
                let collector = self.active.remove(position);
                if result.is_ok() && !collector.issues.is_empty() {
                    let source = provider_name(&feeds()[collector.index].provider);
                    self.warnings
                        .push(format!("{source}：{}", collector.issue_summary()));
                }
                self.failed += usize::from(result.is_err());
                results.push((collector.index, result));
                self.completed += 1;
            } else {
                position += 1;
            }
        }
        // Four live sockets; at most two per publisher. Refill every tick, without waiting for
        // a whole batch, so one failing site cannot hold up the remaining sources.
        while self.active.len() < 4 {
            let next = self.queue.iter().position(|&index| {
                self.active
                    .iter()
                    .filter(|c| feeds()[c.index].provider == feeds()[index].provider)
                    .count()
                    < 2
            });
            let Some(position) = next else {
                break;
            };
            let index = self.queue.remove(position).unwrap();
            match Collector::new(index, self.detail_target, &self.categories) {
                Ok(collector) => self.active.push(collector),
                Err(error) => {
                    results.push((index, Err(error)));
                    self.completed += 1;
                    self.failed += 1;
                }
            }
        }
        results
    }
}
