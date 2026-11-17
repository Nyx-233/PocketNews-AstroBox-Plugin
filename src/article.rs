// Extract public article text and publication metadata without executing page scripts.
use crate::{news, sources};
use scraper::{ElementRef, Html, Selector, node::Node};
use serde_json::Value;

#[derive(Default, Debug)]
pub struct Extracted {
    pub body: String,
    pub published_at: Option<u64>,
    pub channels: Vec<String>,
}
impl Extracted {
    fn merge(&mut self, other: Self) {
        if self.body.is_empty() {
            self.body = other.body;
        }
        self.published_at = self.published_at.or(other.published_at);
        for channel in other.channels {
            if !self.channels.contains(&channel) {
                self.channels.push(channel);
            }
        }
    }
}
fn select(query: &str) -> Selector {
    Selector::parse(query).expect("Bundled article selector")
}
fn clean(element: ElementRef<'_>) -> String {
    let mut output = String::new();
    for node in element.descendants() {
        if node.ancestors().filter_map(ElementRef::wrap).any(|e| {
            matches!(
                e.value().name(),
                "script" | "style" | "nav" | "aside" | "noscript" | "video" | "audio"
            ) || ["advert", "related", "recommend", "share", "comment"]
                .iter()
                .any(|s| e.value().attr("class").unwrap_or("").contains(s))
                || e.value().attr("hidden").is_some()
                || e.value()
                    .attr("style")
                    .unwrap_or("")
                    .replace(' ', "")
                    .contains("display:none")
        }) {
            continue;
        }
        match node.value() {
            Node::Text(text) => output.push_str(text),
            Node::Element(element)
                if matches!(
                    element.name(),
                    "p" | "div" | "br" | "li" | "blockquote" | "h2" | "h3" | "section"
                ) =>
            {
                output.push('\n')
            }
            _ => {}
        }
    }
    news::plain(&output)
}
fn html_text(value: &str) -> String {
    let document = Html::parse_fragment(value);
    clean(document.root_element())
}
pub fn date(value: &str) -> Option<u64> {
    let normalized = value
        .replace('年', "-")
        .replace('月', "-")
        .replace('日', " ");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let value = normalized.trim();
    let numeric = value.parse::<u64>().ok();
    if let Some(number) = numeric {
        return (number > 0).then_some(if number < 100_000_000_000 {
            number.saturating_mul(1000)
        } else {
            number
        });
    }
    if let Some(value) = sources::timestamp(value, false) {
        return Some(value);
    }
    // RFC3339 producers also emit compact offsets or omit seconds.
    for format in ["%Y-%m-%dT%H:%M%z", "%Y-%m-%dT%H:%M:%S%z"] {
        if let Ok(time) = chrono::DateTime::parse_from_str(value, format) {
            return u64::try_from(time.timestamp_millis()).ok();
        }
    }
    for start in value.char_indices().map(|(i, _)| i) {
        for length in [19, 16, 10] {
            if let Some(text) = value.get(start..start + length) {
                if text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                    if let Some(time) = sources::timestamp(text, false) {
                        return Some(time);
                    }
                }
            }
        }
    }
    None
}
fn value_date(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(date)
        .or_else(|| value.as_u64().and_then(|v| date(&v.to_string())))
}
fn object_date(value: &Value) -> Option<u64> {
    [
        "datePublished",
        "publishTime",
        "pubTimeLong",
        "show_time",
        "published_at",
        "publish_time",
        "pubtime",
        "pubTime",
        "createdAt",
        "created_at",
        "created_time",
        "create_time",
        "postdate",
        "ctime",
        "timestamp",
    ]
    .iter()
    .filter_map(|key| value.get(key))
    .find_map(value_date)
}
fn from_json(value: &Value) -> Extracted {
    let mut body = [
        "articleBody",
        "content",
        "content_html",
        "contentHtml",
        "content_text",
        "body",
        "htmlContent",
        "text",
    ]
    .iter()
    .filter_map(|key| value.get(key)?.as_str())
    .map(html_text)
    .find(|s| !s.is_empty())
    .unwrap_or_default();
    // New forum editors keep the complete document beside the legacy HTML field.
    // Read the rendered document, never an image caption or a reply.
    if let Some(format) = value
        .get("format")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
    {
        if let Some(html) = format.get("htmlV3").and_then(Value::as_str) {
            let full = html_text(html);
            if full.chars().count() > body.chars().count() {
                body = full;
            }
        }
    }
    let channels = ["articleSection", "category", "channel", "nodeInfo", "tags"]
        .iter()
        .filter_map(|key| value.get(key))
        .flat_map(crate::classification::metadata)
        .collect();
    Extracted {
        body,
        published_at: object_date(value),
        channels,
    }
}
fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn matching_title(value: &Value, title: &str) -> bool {
    let wanted = normalize(title);
    ["headline", "title", "name", "widgetTitle"]
        .iter()
        .filter_map(|key| value.get(key)?.as_str())
        .any(|found| {
            let found = normalize(found);
            let prefix: String = wanted.chars().take(12).collect();
            !prefix.is_empty()
                && (found.contains(&prefix)
                    || (found.chars().count() >= 8 && wanted.contains(&found)))
        })
}
fn json_article(value: &Value, title: &str, depth: usize) -> Option<Result<Extracted, String>> {
    if depth > 14 {
        return None;
    }
    match value {
        Value::Object(object) => {
            let typed = value.get("@type").is_some_and(|v| {
                v.as_str().is_some_and(|s| {
                    matches!(
                        s,
                        "NewsArticle" | "Article" | "BlogPosting" | "SocialMediaPosting"
                    )
                }) || v.as_array().is_some_and(|values| {
                    values.iter().any(|s| {
                        s.as_str()
                            .is_some_and(|s| matches!(s, "NewsArticle" | "Article" | "BlogPosting"))
                    })
                })
            });
            if typed || matching_title(value, title) {
                if value
                    .get("isAccessibleForFree")
                    .is_some_and(|v| v == false || v == "false")
                {
                    return Some(Err("原文需要付费或订阅，不能补齐全文".into()));
                }
                let article = from_json(value);
                if !article.body.is_empty() {
                    return Some(Ok(article));
                }
            }
            object
                .values()
                .find_map(|v| json_article(v, title, depth + 1))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|v| json_article(v, title, depth + 1)),
        _ => None,
    }
}
fn split_arguments(value: &str) -> Vec<&str> {
    let mut parts = vec![];
    let (mut start, mut depth, mut quoted, mut escaped) = (0, 0, None, false);
    for (i, c) in value.char_indices() {
        if let Some(quote) = quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                quoted = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quoted = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                parts.push(value[start..i].trim());
                return parts;
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(value[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}
// Nuxt 2 serializes literal values as function arguments. Resolve only a timestamp alias;
// no JavaScript is evaluated and unrelated objects are never treated as article text.
fn nuxt_date(script: &str) -> Option<u64> {
    let key = script.find("createTimestamp:")? + "createTimestamp:".len();
    let token: String = script[key..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if token.chars().all(|c| c.is_ascii_digit()) {
        return date(&token);
    }
    let start = script.find("function(")? + "function(".len();
    let end = script[start..].find(')')? + start;
    let index = script[start..end]
        .split(',')
        .position(|name| name.trim() == token)?;
    let args = &script[script.rfind("}(")? + 2..];
    let value = *split_arguments(args).get(index)?;
    date(value.trim_matches('"'))
}
fn next_data(document: &Html) -> Option<Value> {
    let script = document.select(&select("script#__NEXT_DATA__")).next()?;
    serde_json::from_str(&script.inner_html()).ok()
}
pub fn parse(address: &str, title: &str, input: &str) -> Result<Extracted, String> {
    if input.contains("aliyun_waf")
        || input.contains("id=\"captcha\"")
        || input.contains("百度安全验证")
    {
        return Err("原文要求安全验证，匿名请求无法取得正文".into());
    }
    if input.trim_start().starts_with('{') {
        let json: Value = serde_json::from_str(input).map_err(|_| "原文数据格式无效")?;
        return json_article(&json, title, 0)
            .unwrap_or_else(|| Err("原文接口未返回可读取的文章正文".into()));
    }
    let document = Html::parse_document(input);
    let mut found = Extracted::default();
    if let Some(value) = next_data(&document) {
        let path = if address.contains("hupu.com") {
            Some("/props/pageProps/detail/thread")
        } else if address.contains("thepaper.cn") {
            Some("/props/pageProps/detailData/contentDetail")
        } else {
            None
        };
        if let Some(path) = path {
            if let Some(value) = value.pointer(path) {
                let article = from_json(value);
                if article.body.is_empty() {
                    return Err("原文是图片或视频内容，没有可离线保存的文字正文".into());
                }
                found.merge(article);
            }
        }
        if found.body.is_empty() {
            if let Some(article) = json_article(&value, title, 0) {
                found.merge(article?);
            }
        }
    }
    for script in document.select(&select("script[type='application/ld+json']")) {
        if let Ok(json) = serde_json::from_str::<Value>(&script.inner_html()) {
            if let Some(article) = json_article(&json, title, 0) {
                found.merge(article?);
            }
            if found.published_at.is_none() {
                found.published_at = find_json_date(&json, 0);
            }
        }
    }
    for meta in document.select(&select("meta")) {
        let key = meta
            .value()
            .attr("property")
            .or_else(|| meta.value().attr("name"))
            .or_else(|| meta.value().attr("itemprop"))
            .unwrap_or("")
            .to_lowercase();
        let value = meta.value().attr("content").unwrap_or("");
        if [
            "article:published_time",
            "og:release_date",
            "pubdate",
            "publishdate",
            "publish-date",
            "publish_time",
            "datepublished",
            "date",
        ]
        .contains(&key.as_str())
        {
            found.published_at = found.published_at.or_else(|| date(value));
        }
        if matches!(
            key.as_str(),
            "articlesection" | "article:section" | "channel" | "category"
        ) {
            found.channels.push(value.into());
        }
    }
    let selectors = if address.contains("chinanews.com") {
        ".left_zw, .content_maincontent, #content"
    } else if address.contains("people.com.cn") {
        "#rm_txt_zw, #rwb_zw, .rm_txt_con, .text_con_left .box_con"
    } else if address.contains("sputniknews.cn") {
        ".article__body"
    } else if address.contains("gelonghui.com") {
        "article.main-news, .article-with-html"
    } else if address.contains("ifeng.com") {
        "section[aria-label='正文'], #main_content, .main_content, [class*='index_text_']"
    } else if address.contains("36kr.com") {
        ".item-desc, .articleDetailContent, .article-content"
    } else if address.contains("qq.com") {
        ".content-article, .qq_article, #Cnt-Main-Article-QQ, [class*='article-content']"
    } else if address.contains("tieba.baidu.com") {
        ".l_post:first-of-type .d_post_content, .d_post_content"
    } else if address.contains("zaobao.com") || address.contains("zaochenbao.com") {
        ".articleBody, .article-content, .article-body, #article-body, .article-body-container, .content"
    } else if address.contains("toutiao.com") {
        ".article-content, .syl-article-base, .tt-article-content"
    } else {
        "article [itemprop='articleBody'], [itemprop='articleBody'], article .content"
    };
    if found.body.is_empty() {
        // Comma-separated CSS yields document order, not selector priority. A broad outer
        // container must not win over the exact article body and include adjacent news.
        for query in selectors.split(',') {
            if let Some(element) = document.select(&select(query.trim())).next() {
                found.body = clean(element);
                break;
            }
        }
    }
    for time in document.select(&select("[itemprop='datePublished'], time[datetime], .article-info, .news_time_source, .sourceTime, .article-date, .date-source, .time-source, .left-time, #pubtime_baidu, .content_left_time")) {
        if found.published_at.is_some(){break;}
        let value=time.value().attr("datetime").or_else(||time.value().attr("content")).unwrap_or("");
        found.published_at=date(value).or_else(||date(&time.text().collect::<String>()));
    }
    if address.contains("gelonghui.com") {
        for script in document.select(&select("script")) {
            found.published_at = found
                .published_at
                .or_else(|| nuxt_date(&script.inner_html()));
        }
    }
    if address.contains("people.com.cn") {
        for time in document.select(&select(".channel, .art_info, .artInfo")) {
            let value = time.text().collect::<String>();
            if value.contains(':') {
                if let Some(time) = date(&value) {
                    found.published_at = Some(time);
                    break;
                }
            }
        }
    }
    if address.contains("tieba.baidu.com") {
        if let Some(post) = document.select(&select(".l_post[data-field]")).next() {
            if let Ok(value) =
                serde_json::from_str::<Value>(post.value().attr("data-field").unwrap_or(""))
            {
                found.published_at = value
                    .pointer("/content/date")
                    .and_then(value_date)
                    .or(found.published_at);
            }
        }
    }
    if found.body.is_empty() {
        let page_title = document
            .select(&select("title"))
            .next()
            .map(|e| e.text().collect::<String>().to_lowercase())
            .unwrap_or_default();
        if ["登录", "sign in", "login"]
            .iter()
            .any(|s| page_title.contains(s))
            || document
                .select(&select(".login-required, .login-wall, #loginRequired"))
                .next()
                .is_some()
        {
            return Err("原文未返回正文，可能需要登录后访问".into());
        }
        return Err("原文未提供可读取的文字正文，可能是图片、视频或动态加载页面".into());
    }
    Ok(found)
}
fn find_json_date(value: &Value, depth: usize) -> Option<u64> {
    if depth > 5 {
        return None;
    }
    if let Some(time) = value.get("datePublished").and_then(value_date) {
        return Some(time);
    }
    match value {
        Value::Object(o) => o.values().find_map(|v| find_json_date(v, depth + 1)),
        Value::Array(a) => a.iter().find_map(|v| find_json_date(v, depth + 1)),
        _ => None,
    }
}
// Topic pages can link to a concrete public thread. Return its own title and date later;
// the topic creation time must never become the original post's publication time.
pub fn linked_article(address: &str, input: &str) -> Option<(String, String)> {
    if !address.contains("tieba.baidu.com/hottopic/") {
        return None;
    }
    let document = Html::parse_document(input);
    document.select(&select("a[href*='/p/']")).find_map(|a| {
        let link = news::resolve_url(address, a.value().attr("href")?)?;
        let title = a
            .value()
            .attr("title")
            .map(str::to_owned)
            .unwrap_or_else(|| a.text().collect::<String>());
        (news::plain(&title).chars().count() >= 5).then(|| (link, news::plain(&title)))
    })
}
