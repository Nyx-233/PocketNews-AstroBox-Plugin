// Text-only transfer fixture. `--hundred` fills a 100-article batch close to the 512 KB limit.
fn main() {
    let hundred = std::env::args().any(|arg| arg == "--hundred");
    let a = pocket_news::news::Article {
        id: "fixture-1".into(),
        title: "中文传输与保存".into(),
        summary: "可靠接收新闻摘要".into(),
        body: "新闻正文内容。".repeat(3000),
        source: "Fixture".into(),
        url: "https://example.com/fixture".into(),
        category: "tech".into(),
        hot: false,
        published_at: None,
    };
    let articles = if hundred {
        (0..pocket_news::news::MAX_ARTICLES)
            .map(|index| {
                let mut item = a.clone();
                item.id = format!("fixture-{index}");
                item.url = format!("https://example.com/fixture/{index}");
                item.title = format!("第 {} 篇完整新闻", index + 1);
                item.body = "新闻正文内容。".repeat(220);
                item
            })
            .collect()
    } else {
        vec![a]
    };
    let t = pocket_news::transfer::Transfer::new("fixture-device".into(), articles, 1793851200000)
        .unwrap();
    let mut packets = vec![t.handshake()];
    packets.extend(t.messages.clone());
    let batch: serde_json::Value = serde_json::from_str(&t.payload).unwrap();
    println!(
        "{}",
        serde_json::json!({"packets": packets, "batch": batch})
    );
}
