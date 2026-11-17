use pocket_news::{
    news::{self, Article},
    selection::Filter,
    transfer::{Batch, Transfer},
};
use std::collections::HashSet;

fn article(category: &str, index: usize) -> Article {
    let url = format!("https://example.com/{category}/{index}");
    Article {
        id: news::hash(&url),
        title: format!("新闻 {index}"),
        summary: "来源提供的摘要".into(),
        body: format!("第 {index} 篇完整正文，不应被截断。"),
        source: "测试来源".into(),
        url,
        category: category.into(),
        hot: false,
        published_at: Some(index as u64 * 1000),
    }
}
fn categories(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

#[test]
fn restart_preserves_preferences_without_restoring_fetched_news() {
    let mut saved: news::Saved = serde_json::from_value(serde_json::json!({
        "article_count": 45, "preferences": ["china", "world"], "no_images": true,
        "articles": [article("china", 0)], "fetched_at": 123
    }))
    .unwrap();
    assert_eq!(saved.article_count, 45);
    assert!(saved.articles.is_empty());
    assert_eq!(saved.fetched_at, 0);
    saved.articles.push(article("world", 1));
    saved.fetched_at = 456;
    let disk = serde_json::to_value(&saved).unwrap();
    assert!(disk.get("articles").is_none());
    assert!(disk.get("fetched_at").is_none());
    // Older caches stored an image switch; text-only pushes ignore and drop it.
    assert!(disk.get("no_images").is_none());
    let restored: news::Saved = serde_json::from_value(disk).unwrap();
    assert_eq!(restored.preferences, vec!["china", "world"]);
    assert!(restored.articles.is_empty());
    // Older caches without folding fields open the settings card and fold the news list.
    assert!(restored.settings_open && !restored.articles_open);
}

#[test]
fn every_user_setting_survives_a_restart() {
    let mut saved = news::Saved::default();
    saved.article_count = 70;
    saved.preferences = categories(&["tech", "finance"]);
    saved.source_preferences = categories(&["ithome", "cls"]);
    saved.settings_open = false;
    saved.articles_open = true;
    let restored: news::Saved =
        serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    assert_eq!(restored.article_count, 70);
    assert_eq!(restored.preferences, saved.preferences);
    assert_eq!(restored.source_preferences, saved.source_preferences);
    assert!(!restored.settings_open && restored.articles_open);
    // The chosen band is a per-session choice and is never written to the cache.
    let disk = serde_json::to_value(&restored).unwrap();
    assert!(disk.get("device").is_none() && disk.get("sources_open").is_none());
}

#[test]
fn collection_count_handles_host_values_and_balances_up_to_one_hundred() {
    for (payload, expected) in [
        ("1", 1),
        ("100", 100),
        ("120", 100),
        ("0", 1),
        ("-8", 1),
        (r#"{"value":[47]}"#, 47),
        (r#"{"target":{"value":"63"}}"#, 63),
        (r#"{"detail":{"value":25}}"#, 25),
        (r#""42""#, 42),
    ] {
        assert_eq!(news::article_count(payload), Some(expected));
    }
    for payload in [
        "",
        "null",
        "NaN",
        "Infinity",
        "oops",
        "[]",
        r#"{"value":""}"#,
    ] {
        assert_eq!(news::article_count(payload), None);
    }
    let input: Vec<_> = ["china", "world", "tech"]
        .into_iter()
        .flat_map(|category| (0..100).map(move |i| article(category, i)))
        .collect();
    for count in [1, 17, 30, 100] {
        let result = news::collect_batch_limit(
            input.clone(),
            &categories(&["china", "world", "tech"]),
            count,
        );
        assert_eq!(result.len(), count);
        let sizes: Vec<_> = ["china", "world", "tech"]
            .iter()
            .map(|c| result.iter().filter(|a| a.category == *c).count())
            .collect();
        assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
    }
    assert_eq!(
        news::collect_batch_limit(input, &categories(&["tech"]), 1000).len(),
        100
    );
}
fn selected(values: &[&Article]) -> HashSet<String> {
    values.iter().map(|a| a.id.clone()).collect()
}

#[test]
fn collecting_thirty_balances_categories_and_keeps_the_newest_items() {
    let mut input: Vec<_> = (0..50).map(|i| article("tech", i)).collect();
    input.extend((0..50).map(|i| article("china", i)));
    input.extend((0..50).map(|i| article("world", i)));
    let result = news::collect_batch(input, &categories(&["china", "world", "tech"]));
    assert_eq!(result.len(), 30);
    for category in ["china", "world", "tech"] {
        let items: Vec<_> = result.iter().filter(|a| a.category == category).collect();
        assert_eq!(items.len(), 10);
        assert_eq!(items[0].published_at, Some(49000));
        assert_eq!(items[9].published_at, Some(40000));
    }
    assert_eq!(result[0].category, "china");
    assert_eq!(result[1].category, "world");
    assert_eq!(result[2].category, "tech");
}
#[test]
fn rss_and_json_parsers_supply_a_full_thirty_article_batch() {
    let items: String = (0..40)
        .map(|i| format!("<item><title>新闻{i}</title><link>https://example.com/rss/{i}</link><description>来源正文{i}</description></item>"))
        .collect();
    let rss = news::parse_rss(
        &format!("<rss><channel>{items}</channel></rss>"),
        "RSS",
        "tech",
    )
    .unwrap();
    assert_eq!(news::collect_batch(rss, &categories(&["tech"])).len(), 30);
    let items: Vec<_> = (0..30)
        .map(|i| {
            serde_json::json!({
                "title": format!("财经新闻{i}"), "content_text": format!("快讯正文{i}"),
                "uri": format!("https://example.com/finance/{i}"), "display_time": 1000 + i
            })
        })
        .collect();
    let json = serde_json::json!({"data": {"items": items}}).to_string();
    let parsed = news::parse_json(&json, "华尔街见闻", "finance").unwrap();
    let result = news::collect_batch(parsed, &categories(&["finance"]));
    assert_eq!(result.len(), 30);
    assert_eq!(result[0].body, "快讯正文29");
}

#[test]
fn a_single_source_can_supply_thirty_without_the_old_twelve_item_cutoff() {
    let result = news::collect_batch(
        (0..60).map(|i| article("finance", i)).collect(),
        &categories(&["finance"]),
    );
    assert_eq!(result.len(), 30);
    assert_eq!(result.first().unwrap().published_at, Some(59000));
    assert_eq!(result.last().unwrap().published_at, Some(30000));
}
#[test]
fn collectors_deduplicate_and_use_other_categories_when_one_runs_short() {
    let mut input: Vec<_> = (0..40).map(|i| article("tech", i)).collect();
    input.extend(input.clone());
    input.push(article("china", 1));
    input.push(article("sports", 100));
    let result = news::collect_batch(input, &categories(&["china", "tech", "tech"]));
    assert_eq!(result.len(), 30);
    assert_eq!(result.iter().filter(|a| a.category == "china").count(), 1);
    assert_eq!(result.iter().filter(|a| a.category == "tech").count(), 29);
    let ids: HashSet<_> = result.iter().map(|a| &a.id).collect();
    assert_eq!(ids.len(), 30);
}
#[test]
fn shortages_remain_real_and_empty_feeds_do_not_invent_news() {
    let a = article("tech", 1);
    let result = news::collect_batch(vec![a.clone(), a], &categories(&["tech"]));
    assert_eq!(result.len(), 1);
    assert!(news::collect_batch(vec![], &categories(&["tech"])).is_empty());
    assert!(news::collect_batch(vec![article("tech", 1)], &[]).is_empty());
}
#[test]
fn filtering_never_changes_selection_or_source_articles() {
    let list = vec![article("tech", 1), article("china", 2), article("tech", 3)];
    let chosen = selected(&[&list[0], &list[1]]);
    assert_eq!(Filter::All.visible(&list, &chosen).len(), 3);
    assert_eq!(Filter::Selected.visible(&list, &chosen).len(), 2);
    assert_eq!(
        Filter::Category("tech".into())
            .visible(&list, &chosen)
            .len(),
        2
    );
    assert_eq!(chosen.len(), 2);
    assert_eq!(list.len(), 3);
    assert_eq!(Filter::from_id("all"), Some(Filter::All));
    assert_eq!(Filter::from_id("selected"), Some(Filter::Selected));
    assert_eq!(
        Filter::from_id("tech"),
        Some(Filter::Category("tech".into()))
    );
    assert_eq!(Filter::from_id("unknown"), None);
}
#[test]
fn select_all_and_invert_only_touch_visible_articles() {
    let list = vec![article("tech", 1), article("china", 2), article("tech", 3)];
    let filter = Filter::Category("tech".into());
    let mut chosen = selected(&[&list[0], &list[1]]);
    filter.toggle_all(&list, &mut chosen);
    assert_eq!(chosen, selected(&[&list[0], &list[1], &list[2]]));
    filter.toggle_all(&list, &mut chosen);
    assert_eq!(chosen, selected(&[&list[1]]));
    filter.invert(&list, &mut chosen);
    assert_eq!(chosen.len(), 3);
    filter.invert(&list, &mut chosen);
    assert_eq!(chosen, selected(&[&list[1]]));
    Filter::Category("sports".into()).toggle_all(&list, &mut chosen);
    assert_eq!(chosen, selected(&[&list[1]]));
}
#[test]
fn selected_filter_bulk_actions_can_clear_the_view_safely() {
    let list = vec![article("tech", 1), article("china", 2), article("tech", 3)];
    let mut chosen = selected(&[&list[0], &list[1]]);
    Filter::Selected.toggle_all(&list, &mut chosen);
    assert!(chosen.is_empty());
    Filter::Selected.invert(&list, &mut chosen);
    assert!(chosen.is_empty());
    Filter::All.toggle_all(&list, &mut chosen);
    assert_eq!(chosen.len(), 3);
    Filter::Selected.invert(&list, &mut chosen);
    assert!(chosen.is_empty());
}
#[test]
fn transfer_accepts_one_hundred_complete_articles_but_still_enforces_size_and_count() {
    let list: Vec<_> = (0..100).map(|i| article("tech", i)).collect();
    let task = Transfer::new("band".into(), list.clone(), 42).unwrap();
    let batch: Batch = serde_json::from_str(&task.payload).unwrap();
    assert_eq!(batch.articles.len(), 100);
    assert_eq!(batch.articles[99].body, list[99].body);
    assert!(task.messages.iter().all(|packet| packet.len() <= 18000));
    assert!(
        Transfer::new(
            "band".into(),
            (0..101).map(|i| article("tech", i)).collect(),
            43
        )
        .is_err()
    );
    let mut oversized = list;
    for a in &mut oversized {
        a.body = "完整长文".repeat(1000);
    }
    assert!(
        Transfer::new("band".into(), oversized, 44)
            .err()
            .unwrap()
            .contains("512 KB")
    );
}

#[test]
fn source_selection_removes_only_the_six_retired_providers() {
    use pocket_news::sources;
    let available: HashSet<_> = sources::providers().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        available,
        HashSet::from([
            "ithome",
            "wallstreetcn",
            "cls",
            "jin10",
            "zaobao",
            "36kr",
            "hupu",
            "dongqiudi",
            "thepaper",
            "sputniknewscn",
            "gelonghui",
            "ifeng",
            "tencent",
            "chinanews",
            "people"
        ])
    );
    let removed = categories(&["weibo", "zhihu", "tieba", "toutiao", "xueqiu", "baidu"]);
    assert!(sources::normalize_selection(&removed).is_empty());
    assert!(sources::jobs(&categories(&["hot", "tech", "finance"]), &removed).is_empty());
    assert!(
        sources::feeds()
            .iter()
            .all(|feed| !removed.contains(&feed.provider))
    );
    let saved = categories(&["weibo", "thepaper", "chinanews", "people", "cls", "ithome"]);
    assert_eq!(
        sources::normalize_selection(&saved),
        categories(&["ithome", "thepaper", "cls", "chinanews", "people"])
    );
    assert!(sources::normalize_selection(&[]).is_empty());
    let jobs = sources::jobs(
        &categories(&["tech"]),
        &categories(&["weibo", "ithome", "cls"]),
    );
    assert_eq!(jobs.len(), 2);
    assert!(
        jobs.iter()
            .all(|i| matches!(sources::feeds()[*i].provider.as_str(), "ithome" | "cls"))
    );
    assert!(sources::jobs(&[], &categories(&["cls"])).is_empty());
    assert_eq!(sources::timestamp("2025-06-05", false), Some(1749052800000));
}

#[test]
fn dated_flash_sources_preserve_the_complete_message_and_publication_date() {
    use pocket_news::sources;
    for (id, input, url) in [
        (
            "cls",
            r#"{"data":{"roll_data":[{"id":1,"title":"科技公司公布财报","brief":"净利润增长20%。","ctime":1794729600}]}}"#,
            "https://www.cls.cn/detail/1",
        ),
        (
            "jin10",
            r#"var newest = [{"id":1,"time":"2026-11-15 12:00:00","data":{"title":"央行公布利率决议","content":"央行宣布降息25个基点。"}}];"#,
            "https://flash.jin10.com/detail/1",
        ),
    ] {
        let index = sources::feeds().iter().position(|f| f.id == id).unwrap();
        let items = sources::readable_articles(sources::parse_response(index, input).unwrap());
        assert_eq!(items.len(), 1, "{id}");
        assert_eq!(items[0].url, url);
        assert!(items[0].published_at.is_some());
        assert_eq!(items[0].category, "finance");
    }
    let mut complete = article("tech", 1);
    complete.title = "科研团队研发量子芯片".into();
    complete.body = "研发团队公布了完整的芯片测试结果。".into();
    let mut missing_date = complete.clone();
    missing_date.published_at = None;
    let mut empty_body = complete.clone();
    empty_body.body.clear();
    let mut teaser = complete.clone();
    teaser.body = "本文提供部分介绍，点击阅读全文".into();
    let mut ellipsis = complete.clone();
    ellipsis.body = "研发团队公布部分信息...".into();
    let mut uncertain = complete.clone();
    uncertain.title = "今日消息".into();
    uncertain.body = "更多消息，敬请关注。".into();
    uncertain.category = "society".into();
    let result = sources::readable_articles(vec![
        complete.clone(),
        missing_date,
        empty_body,
        teaser,
        ellipsis,
        uncertain,
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, complete.body);
}

#[test]
fn atom_feeds_preserve_real_content_and_publication_dates() {
    let input = r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>原始标题</title><link rel="alternate" href="https://example.com/1"/><updated>2026-11-10T12:00:00Z</updated><content type="html">&lt;p&gt;来源正文&lt;/p&gt;</content></entry></feed>"#;
    let result = news::parse_rss(input, "Product Hunt", "tech").unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, "来源正文");
    assert_eq!(result[0].url, "https://example.com/1");
    assert!(result[0].published_at.is_some());
}

#[test]
fn cross_source_duplicates_merge_before_counting_and_keep_the_complete_report() {
    let mut short = article("tech", 1);
    short.title = "【快讯】某研究团队成功研发新一代高性能量子计算芯片".into();
    short.body = format!(
        "{}\n\n该来源仅提供标题，请在手机打开原文阅读。",
        short.title
    );
    short.hot = true;
    let mut complete = article("tech", 2);
    complete.title = "某研究团队成功研发新一代高性能量子计算芯片！".into();
    complete.body =
        "这是来自另一来源的完整报道，包含研究团队介绍、研发过程、芯片性能和后续应用计划。".into();
    complete.source = "Second publisher".into();
    let mut extended = article("tech", 3);
    extended.title = "某研究团队成功研发了新一代高性能量子计算芯片".into();
    let unique = article("tech", 4);
    let result = news::collect_batch_limit(
        vec![short, complete.clone(), extended, unique],
        &categories(&["tech", "hot"]),
        3,
    );
    assert_eq!(result.len(), 2);
    let merged = result.iter().find(|a| a.id == complete.id).unwrap();
    assert_eq!(merged.body, complete.body);
    assert_eq!(merged.source, complete.source);
    assert_eq!(merged.url, complete.url);
    assert!(merged.hot);
    assert_eq!(
        Transfer::new("band".into(), result, 1)
            .unwrap()
            .payload
            .matches("量子计算芯片")
            .count(),
        1
    );
}

#[test]
fn duplicate_identity_ignores_tracking_but_preserves_distinct_claims_dates_and_subjects() {
    let title = "某研究团队成功研发新一代高性能量子计算芯片";
    let mut input = vec![];
    for (i, value) in [
        title,
        title,
        "另一研究团队成功研发新一代高性能量子计算芯片",
        "某研究团队成功研发新一代高性能光学计算芯片",
        "某研究团队未成功研发新一代高性能量子计算芯片",
        "某公司发布季度财报净利润同比增长1.5亿元",
        "某公司发布季度财报净利润同比增长15亿元",
    ]
    .into_iter()
    .enumerate()
    {
        let mut a = article("tech", 100 + i);
        a.title = value.into();
        if i == 1 {
            a.published_at = Some(3 * 24 * 60 * 60 * 1000);
        }
        input.push(a);
    }
    assert_eq!(news::deduplicate(input).len(), 7);
    let mut original = article("finance", 1);
    original.url = "https://www.example.com/story?id=1&utm_source=feed#top".into();
    let mut copy = article("finance", 2);
    copy.url = "https://example.com/story?from=share&id=1".into();
    copy.hot = true;
    let mut other = article("finance", 3);
    other.url = "https://example.com/story?id=2".into();
    let result = news::deduplicate(vec![original, copy, other]);
    assert_eq!(result.len(), 2);
    assert!(result[0].hot);
    let mut agency = article("tech", 20);
    agency.title = "研究团队取得最新进展".into();
    agency.body =
        "研究团队介绍了新一代量子计算芯片的技术细节、研发过程、性能指标与应用计划。".repeat(5);
    let mut reprint = article("tech", 21);
    reprint.title = "新一代芯片技术细节公布".into();
    reprint.body = format!("新华社11月15日电，{}", agency.body);
    assert_eq!(news::deduplicate(vec![agency, reprint]).len(), 1);
}

#[test]
fn article_extraction_uses_original_dates_and_excludes_replies_or_paywalled_text() {
    use pocket_news::article;
    let timestamp = pocket_news::sources::timestamp("2026-11-13 09:30:00", false).unwrap();
    let people = article::parse("https://military.people.com.cn/n1/2026/a.html", "军训报道", r#"<meta name="publishdate" content="2026-11-13"><div class="channel">2026年11月13日 09:30 来源：报纸</div><div class="rm_txt_con"><p>原始完整报道第一段。</p><p>原始完整报道第二段。</p><aside>不应混入的推荐</aside></div>"#).unwrap();
    assert_eq!(people.published_at, Some(timestamp));
    assert_eq!(people.body, "原始完整报道第一段。\n\n原始完整报道第二段。");
    let hupu=article::parse("https://bbs.hupu.com/123.html","原帖标题",r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"detail":{"thread":{"title":"原帖标题","content":"<p>这是原帖完整正文。</p>","createdAt":1794738300000},"replies":[{"content":"不应混入的回复"}]}}}}</script>"#).unwrap();
    assert_eq!(hupu.body, "这是原帖完整正文。");
    assert_eq!(hupu.published_at, Some(1794738300000));
    let formatted = serde_json::json!({"props":{"pageProps":{"detail":{"thread":{"title":"富文本原帖","content":"<p></p>","createdAt":1794738300000_u64,"format":serde_json::json!({"htmlV3":"<p>新编辑器保存的完整正文。</p>"}).to_string()}}}}});
    let formatted = article::parse(
        "https://bbs.hupu.com/124.html",
        "富文本原帖",
        &format!("<script id=\"__NEXT_DATA__\" type=\"application/json\">{formatted}</script>"),
    )
    .unwrap();
    assert_eq!(formatted.body, "新编辑器保存的完整正文。");
    let image_only = article::parse(
        "https://bbs.hupu.com/123.html",
        "图片帖",
        r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"detail":{"thread":{"title":"图片帖","content":"<p><img src='https://example.com/image.jpg'></p>","createdAt":1794738300000},"replies":[{"content":"不能把回复当成正文"}]}}}}</script>"#,
    );
    assert!(image_only.unwrap_err().contains("图片或视频"));
    assert!(article::parse("https://example.com/news","付费报道",r#"<script type="application/ld+json">{"@type":"NewsArticle","headline":"付费报道","isAccessibleForFree":false,"articleBody":"不能使用隐藏的付费正文","datePublished":"2026-11-15T08:00:00Z"}</script>"#).unwrap_err().contains("付费或订阅"));
    assert!(
        article::parse(
            "https://example.com/news",
            "报道",
            "<title>百度安全验证</title>"
        )
        .unwrap_err()
        .contains("安全验证")
    );
}

#[test]
fn embedded_lists_and_nuxt_dates_are_read_without_executing_javascript() {
    use pocket_news::{article, sources};
    let exact=article::parse("https://www.chinanews.com.cn/jk/2026/11-13/1.shtml","医疗报道",r#"<div class="content_maincontent"><div id="BaiduSpider" style="display:none"><span id="pubtime_baidu">2026-11-13 16:18:19</span></div><div class="left_zw"><p>真正的文章完整正文。</p></div><div class="intermoren_right">不能混入的其他新闻和日期。</div></div>"#).unwrap();
    assert_eq!(exact.body, "真正的文章完整正文。");
    assert_eq!(
        exact.published_at,
        sources::timestamp("2026-11-13 16:18:19", false)
    );
    let mixed=article::parse("https://www.chinanews.com.cn/jk/2026/11-13/1.shtml","医疗报道",r#"<script type="application/ld+json">{"@type":"NewsArticle","headline":"医疗报道","articleBody":"医疗报道的完整文字正文。"}</script><div class="left-time">2026年11月13日 19:19 来源：中国新闻网</div>"#).unwrap();
    assert_eq!(mixed.body, "医疗报道的完整文字正文。");
    assert_eq!(
        mixed.published_at,
        sources::timestamp("2026-11-13 19:19", false)
    );
    let index = sources::feeds()
        .iter()
        .position(|f| f.id == "36kr")
        .unwrap();
    let input = r#"<script>function f(){return window.initialState.navigator;} window.initialState = {"newsflashCatalogData":{"data":{"newsflashList":{"data":{"itemList":[{"itemId":123,"templateMaterial":{"widgetTitle":"公司公布财报","widgetContent":"公司公布完整季度财报，净利润增长20%。","publishTime":1794738300000}}]}}}}}; console.log(window.initialState.navigator);</script>"#;
    let items = sources::parse_response(index, input).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].published_at, Some(1794738300000));
    assert!(items[0].body.contains("完整季度财报"));
    let phoenix = sources::feeds()
        .iter()
        .position(|f| f.id == "ifeng")
        .unwrap();
    let input = r#"<script>var allData={"hotNews1":[{"title":"公司公布财报","url":"https://finance.ifeng.com/c/1","newsTime":"2026-11-15 08:00:00"}]};if(typeof allData==='object'){for(var key in allData){}}allData.IE78=true;</script>"#;
    assert_eq!(sources::parse_response(phoenix, input).unwrap().len(), 1);
    let nuxt = r#"<article class="main-news"><p>公司宣布回购股份，完整快讯内容。</p></article><script>window.__NUXT__=(function(a,b,c){return{fetch:{data:{createTimestamp:c,updateTimestamp:c}}}}(false,0,1794738300000));</script>"#;
    let item = article::parse("https://www.gelonghui.com/news/1", "回购股份", nuxt).unwrap();
    assert_eq!(item.published_at, Some(1794738300000));
    assert_eq!(item.body, "公司宣布回购股份，完整快讯内容。");
    let atom = r#"<feed><entry><title>公告</title><link href="https://example.com/1"/><published>2026-11-13T01:00:00Z</published><updated>2026-11-15T01:00:00Z</updated><content>完整正文</content></entry></feed>"#;
    assert_eq!(
        news::parse_rss(atom, "RSS", "").unwrap()[0].published_at,
        sources::timestamp("2026-11-13T01:00:00Z", false)
    );
}
