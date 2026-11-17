// Classify the article, not its publisher. Channel names are stronger evidence than topic tags.
use serde_json::Value;
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct Evidence<'a> {
    pub channels: &'a [String],
    pub tags: &'a [String],
    pub fallback: &'a str,
}

pub fn channel(value: &str) -> Option<&'static str> {
    let value = value.trim().to_lowercase();
    let value = value.trim_end_matches("频道").trim_end_matches("新闻");
    Some(match value {
        "国内" | "中国" | "时政" | "政务" | "china" | "domestic" | "politics" | "时事"
        | "中国政库" | "中南海" | "舆论场" | "港台来信" | "台海" | "港澳" | "taiwan" => {
            "china"
        }
        "国际" | "海外" | "世界" | "全球速报" | "澎湃国际" | "world" | "international" => {
            "world"
        }
        "科技" | "科学" | "数码" | "互联网" | "信息技术" | "科普" | "tech" | "technology"
        | "science" | "digital" | "人工智能" | "ai" | "航天" | "通信" | "游戏" => "tech",
        "财经" | "经济" | "金融" | "证券" | "股票" | "基金" | "期货" | "外汇" | "商业" | "产经"
        | "宏观" | "公司" | "房产" | "房地产" | "地产" | "汽车产业" | "能见度" | "牛市点线面"
        | "财经上下游" | "finance" | "business" | "economy" | "stock" | "stocks"
        | "a-stock-channel" | "us-stock-channel" | "hk-stock-channel" | "forex-channel"
        | "goldc-channel" | "oil-channel" | "financing-channel" => "finance",
        "体育" | "足球" | "篮球" | "网球" | "奥运" | "电竞" | "运动赛事" | "sports" | "sport"
        | "football" | "soccer" | "basketball" | "nba" => "sports",
        "娱乐" | "影视" | "电影" | "电视剧" | "明星" | "综艺" | "有戏" | "文娱" | "晚会"
        | "盛典" | "entertainment" | "ent" | "celebrity" | "追光灯" => "entertainment",
        "军事" | "军情" | "国防" | "军闻" | "military" | "defense" => "military",
        "社会" | "法治" | "法制" | "民生" | "社会民生" | "直击现场" | "一号专案" | "澎湃质量观"
        | "society" | "social" | "legal" => "society",
        "文化" | "文史" | "历史" | "艺术" | "文学" | "读书" | "文博" | "非遗" | "culture"
        | "history" | "art" | "arts" => "culture",
        "教育" | "校园" | "高考" | "升学" | "留学" | "基础教育" | "高等教育" | "education"
        | "edu" => "education",
        "健康" | "养生" | "医疗" | "医药健康" | "卫生" | "医药" | "疾病" | "health" | "medical" => {
            "health"
        }
        "生活" | "天气" | "气象" | "旅游" | "美食" | "家居" | "时尚" | "出行" | "life"
        | "lifestyle" | "weather" | "travel" | "food" => "life",
        // A trending list describes popularity, not the subject of an individual article.
        _ => return None,
    })
}

fn channels(value: &str) -> Vec<&'static str> {
    if let Some(id) = channel(value) {
        return vec![id];
    }
    if value.contains(['/', '>']) {
        if let Some(id) = value.split(['/', '>']).filter_map(channel).next_back() {
            return vec![id];
        }
    }
    value
        .split(['/', '>', '·', '-', '_', '|', ',', '，', '、'])
        .filter_map(channel)
        .collect()
}

// Only extract names from known metadata shapes, never arbitrary article fields or numeric IDs.
pub fn metadata(value: &Value) -> Vec<String> {
    fn names(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) if !s.trim().is_empty() => out.push(s.trim().to_owned()),
            Value::Array(values) => values.iter().for_each(|v| names(v, out)),
            Value::Object(object) => {
                for key in [
                    "name",
                    "title",
                    "label",
                    "term",
                    "category_name",
                    "channel_name",
                    "column_name",
                    "subject_name",
                    "tag",
                    "tag_name",
                    "plate_name",
                    "categoryName",
                    "channelName",
                    "columnName",
                    "tagName",
                ] {
                    if let Some(Value::String(s)) = object.get(key) {
                        names(&Value::String(s.clone()), out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    names(value, &mut out);
    out
}

pub fn json_evidence(row: &Value, paths: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    paths
        .split('|')
        .filter(|p| !p.is_empty())
        .filter_map(|p| row.pointer(p))
        .flat_map(metadata)
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

fn url_channel(address: &str) -> Option<&'static str> {
    let url = url::Url::parse(address).ok()?;
    let host = url.host_str()?;
    // Unknown sites may use these words in slugs rather than in actual channel paths.
    if ![
        "chinanews.com.cn",
        "people.com.cn",
        "qq.com",
        "ifeng.com",
        "zaobao.com",
        "zaobao.com.sg",
        "zaochenbao.com",
        "sputniknews.cn",
        "thepaper.cn",
        "cls.cn",
        "wallstreetcn.com",
        "36kr.com",
        "ithome.com",
        "hupu.com",
        "dongqiudi.com",
    ]
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    for segment in segments.iter().take(3) {
        let id = match *segment {
            "cj" | "business" | "finance" | "economy" | "stock" | "fortune" | "house" => "finance",
            "gn" | "china" | "politics" | "domestic" => "china",
            "gj" | "world" | "international" => "world",
            "sh" | "society" | "legal" => "society",
            "ty" | "sports" | "sport" | "nba" | "soccer" | "football" | "basketball" => "sports",
            "yl" | "ent" | "entertainment" => "entertainment",
            "cul" | "culture" | "history" => "culture",
            "mil" | "military" => "military",
            "edu" | "education" => "education",
            "jk" | "health" | "jiankang" => "health",
            "life" | "weather" | "travel" | "fashion" => "life",
            "it" | "tech" | "science" | "digital" | "人工智能" | "ai" | "航天" | "通信"
            | "游戏" => "tech",
            _ => continue,
        };
        return Some(id);
    }
    // Section subdomains are article-level URL evidence; the publisher's main domain is not.
    channel(host.split('.').next()?)
}

// Attribution is shared with the trained tokenizer and duplicate detection.
pub(crate) fn content_text(value: &str) -> String {
    crate::text_model::clean(value)
}

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}
fn generic(text: &str) -> bool {
    text.trim().is_empty()
        || matches!(
            text.trim(),
            "今日消息"
                | "今日快讯"
                | "最新消息"
                | "今日发布"
                | "内容介绍"
                | "更多消息"
                | "今日新闻"
        )
}
// High-specificity event constraints are applied after geographic scope, never before it.
fn event_subject(title: &str) -> Option<&'static str> {
    let fiction = has(
        title,
        &[
            "剧中",
            "剧情",
            "电影讲述",
            "饰演",
            "虚构",
            "游戏中",
            "游戏里",
            "游戏玩法",
            "小说",
        ],
    );
    if !fiction
        && has(
            title,
            &[
                "获刑",
                "判刑",
                "宣判",
                "刑拘",
                "逮捕",
                "被捕",
                "诈骗",
                "校园欺凌",
                "失联",
                "失信",
                "法庭",
                "裁定",
                "上诉",
                "法院",
                "知识产权协同保护",
                "最低工资",
            ],
        )
    {
        return Some("society");
    }
    if !fiction
        && (has(title, &["消防", "救援", "被困", "火灾", "地震", "海啸"])
            || title.contains("医院") && title.contains("死亡"))
    {
        return Some("society");
    }
    if has(
        title,
        &[
            "招生",
            "高考",
            "中考",
            "录取",
            "学费",
            "助学",
            "教育改革",
            "乡村学校",
        ],
    ) || title.contains("考生") && title.contains("作弊")
    {
        return Some("education");
    }
    if has(title, &["冰箱", "洗衣机", "大路灯"]) && has(title, &["直降", "新低", "优惠", "选购"])
    {
        return Some("life");
    }
    if title.starts_with("iphone") && has(title, &["不用抢", "国补", "优惠", "补贴", "新低"])
    {
        return Some("tech");
    }
    let numeric = title.chars().any(|c| c.is_ascii_digit());
    if numeric
        && has(title, &["市场", "行业", "协会", "乘联", "统计", "全社会"])
        && has(
            title,
            &[
                "同比", "环比", "份额", "占比", "平均", "销量", "零售", "产量", "出口",
            ],
        )
    {
        return Some("finance");
    }
    for clause in title.split([',', '。', ';', '!']) {
        if !fiction
            && has(
                clause,
                &[
                    "财报",
                    "净利润",
                    "营收",
                    "股价",
                    "股指",
                    "市值",
                    "ipo",
                    "融资",
                    "期货",
                    "存款利率",
                    "收益率",
                    "保费",
                    "保单",
                    "保险赔款",
                    "准备金率",
                    "h股",
                    "联交所",
                ],
            )
        {
            return Some("finance");
        }
        if has(
            clause,
            &[
                "手机",
                "平板",
                "电脑",
                "电视盒",
                "学习机",
                "手柄",
                "usb",
                "充电头",
                "充电器",
                "电子墨水",
                "遥控器",
                "处理器",
                "芯片",
                "显卡",
                "操作系统",
                "固件",
                "软件",
                "编程",
                "开源",
                "量子",
                "算力",
                "具身智能",
                "数据标准",
                "鸿蒙",
                "harmonyos",
                "新车",
                "续航",
            ],
        ) && has(
            clause,
            &[
                "推出", "发布", "首发", "支持", "改造", "升级", "搭载", "辅助", "开发", "研发",
                "首秀", "重返", "研究", "成果", "制定", "降价", "新低", "首发", "亮相", "续航",
                "补丁",
            ],
        ) {
            return Some("tech");
        }
    }
    if has(
        title,
        &["天气", "气象", "暴雨预警", "降温", "寒潮", "冷空气"],
    ) {
        return Some("life");
    }
    if has(
        title,
        &[
            "健康提示",
            "预防流感",
            "新药",
            "诊疗",
            "治疗",
            "医生",
            "医学",
        ],
    ) {
        return Some("health");
    }
    if has(
        title,
        &[
            "新书",
            "诗词",
            "考古",
            "文物",
            "古籍",
            "非遗",
            "书法",
            "阅读推广",
            "书香",
            "文学奖",
        ],
    ) {
        return Some("culture");
    }
    if has(
        title,
        &[
            "国家队",
            "女排",
            "男足",
            "国乒",
            "电竞",
            "edg",
            "曼联",
            "梅西",
            "c罗",
            "中足联",
        ],
    ) {
        return Some("sports");
    }
    if has(
        title,
        &[
            "军舰", "海军", "空军", "部队", "国防", "军队", "军演", "实弹", "导弹", "军用", "战机",
        ],
    ) {
        return Some("military");
    }
    if has(
        title,
        &["电视剧", "古装剧", "综艺", "演唱会", "票房", "脱口秀"],
    ) {
        return Some("entertainment");
    }
    None
}
fn resolved(title: &str, body: &str) -> (&'static str, bool, f32) {
    let prediction = crate::text_model::predict(title, body);
    let title = content_text(title);
    let lead: String = content_text(body).chars().take(320).collect();
    let main = if generic(&title) {
        lead.as_str()
    } else {
        title.as_str()
    };
    let anchor = crate::text_model::scope_anchor(main, &lead);
    if anchor == Some("foreign") {
        return ("world", true, 1.0);
    }
    if let Some(subject) = event_subject(main) {
        return (subject, true, 1.0);
    }
    if generic(main)
        || !main.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
            && main.split_whitespace().count() <= 2
            && anchor.is_none()
    {
        return (prediction.category, false, 0.0);
    }
    if anchor == Some("domestic") {
        return (
            prediction.topic,
            prediction.topic_margin >= 0.45,
            prediction.topic_margin,
        );
    }
    (prediction.category, prediction.reliable, prediction.margin)
}
pub fn has_reliable_subject(title: &str, body: &str, _url: &str, id: &str) -> bool {
    let (category, reliable, _) = resolved(title, body);
    category == id && reliable
}
pub fn classify(
    title: &str,
    body: &str,
    url: &str,
    explicit: &[String],
    tags: &[String],
    fallback: &str,
) -> &'static str {
    let (category, reliable, margin) = resolved(title, body);
    if reliable || margin > 0.2 {
        return category;
    }
    let unique = |values: &[String]| {
        let candidates: HashSet<_> = values.iter().flat_map(|s| channels(s)).collect();
        (candidates.len() == 1).then(|| *candidates.iter().next().unwrap())
    };
    unique(explicit)
        .or_else(|| unique(tags))
        .or_else(|| url_channel(url))
        .or_else(|| channel(fallback))
        .unwrap_or(category)
}
