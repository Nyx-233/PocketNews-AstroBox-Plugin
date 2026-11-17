use pocket_news::{
    news,
    transfer::{Reply, Transfer},
};
use serde_json::{Value, json};
fn article() -> news::Article {
    news::parse_rss(r#"<rss><channel><item><title><![CDATA[中文测试 &amp; 新闻]]></title><link>https://example.com/news</link><description><![CDATA[<p>第一段内容</p><p>第二段内容</p>]]></description><pubDate>Thu, 5 Nov 2026 12:00:00 +0800</pubDate></item></channel></rss>"#, "News", "tech").unwrap().remove(0)
}
#[test]
fn rss_preserves_content_and_date_and_removes_markup() {
    let a = article();
    assert_eq!(a.title, "中文测试 & 新闻");
    assert_eq!(a.body, "第一段内容\n\n第二段内容");
    assert_eq!(a.published_at, Some(1793851200000));
    assert_eq!(news::plain("hello😀 &nbsp; world"), "hello   world");
    assert!(news::parse_rss("<html/>", "News", "tech").is_err());
}
#[test]
fn newsnow_collectors_keep_source_and_do_not_invent_body_or_date() {
    let list = news::parse_json(r#"{"data":{"items":[{"title":"","content_text":"财经快讯","uri":"https://example.com/finance","display_time":123}]}}"#, "华尔街见闻", "finance").unwrap();
    assert_eq!(list[0].body, "财经快讯");
    assert_eq!(list[0].published_at, Some(123000));
}
fn reply(raw: &str) -> Value {
    // Exercise the same host -> native quick-app envelope -> packet path as the device.
    let event = json!({
        "addr":"band-10", "pkgName":"com.smalljelly.news",
        "payloadText":json!({"msg":raw}).to_string(),
        "payloadHex":raw.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>()
    });
    pocket_news::transfer::decode_message(&event.to_string()).unwrap()
}
fn advance_to_data(t: &mut Transfer, ranges: Value) {
    let id = t.batch_id.clone();
    for (index, msg) in [
        json!({"operation":"ack","itemType":"begin"}),
        json!({"operation":"ack","itemType":"file","fileIndex":0}),
        json!({"operation":"status","status":"ready"}),
        json!({"fileIndex":0,"status":"resume","window":4,"receivedRanges":ranges}),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(t.next_packet().unwrap().0, index);
        assert!(t.next_packet().is_none());
        let mut msg = msg;
        msg["kind"] = json!(if index == 3 { "P" } else { "T" });
        msg["themeId"] = json!(id);
        assert!(matches!(t.reply(&msg), Reply::Next));
    }
}
#[test]
fn corona_envelopes_and_base91_golden_vector() {
    use pocket_news::transfer::{decode_message, encode_base91};
    assert_eq!(encode_base91(b"test"), "fPNKd");
    let text = "H{\"version\":2,\"type\":\"response\",\"replyTo\":\"test\",\"maxTextChars\":18000,\"maxWindow\":4}";
    for raw in [
        text.to_string(),
        json!({"msg":text}).to_string(),
        json!({"data":json!({"msg":text}).to_string()}).to_string(),
        json!({"payload":text.as_bytes()}).to_string(),
        // AstroBox's actual InterconnectMessage event wraps the quick-app envelope.
        json!({
            "addr":"band-10", "pkgName":"com.smalljelly.news",
            "payloadText":json!({"msg":text}).to_string(),
            "payloadHex":text.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>()
        })
        .to_string(),
    ] {
        assert_eq!(decode_message(&raw), decode_message(text));
    }
    for text in [
        "bad",
        "Azzzz0000",
        "A0000",
        "C00000",
        "😀00000000",
        "[]",
        "H[]",
        r#"{"payloadText":"bad","payloadHex":"00"}"#,
        r#"{"payloadText":null}"#,
    ] {
        assert!(decode_message(text).is_none());
    }
    assert_eq!(reply("A000000ff")["chunkIndex"], 255);
}
#[test]
fn four_packet_window_matches_only_outstanding_acks_and_retries_missing_chunks() {
    let mut a = article();
    a.body = "长文😀".repeat(7000);
    let mut t = Transfer::new("band-10".into(), vec![a], 1234567).unwrap();
    assert!(t.messages.iter().all(|p| p.len() <= 18000));
    advance_to_data(&mut t, json!([]));
    assert!(matches!(t.reply(&reply("A00000000")), Reply::Ignored));
    let first: Vec<_> = (0..4).map(|_| t.next_packet().unwrap().0).collect();
    assert_eq!(first, [4, 5, 6, 7]);
    assert!(t.next_packet().is_none());
    assert!(matches!(t.reply(&reply("A00000005")), Reply::Ignored));
    assert!(matches!(t.reply(&reply("A00000002")), Reply::Next));
    assert!(matches!(t.reply(&reply("A00000002")), Reply::Ignored));
    assert_eq!(t.next_packet().unwrap().0, 8);
    assert!(t.next_packet().is_none());
    assert!(t.retry_pending());
    let retried: Vec<_> = (0..4).map(|_| t.next_packet().unwrap().0).collect();
    assert_eq!(retried, [4, 5, 7, 8]);
    t.paused = true;
    assert!(matches!(t.reply(&reply("A00000000")), Reply::Ignored));
}
#[test]
fn resume_negotiates_sparse_ranges_and_requires_final_durable_receipt() {
    let mut a = article();
    a.body = "长篇新闻".repeat(7000);
    let mut t = Transfer::new("band".into(), vec![a], 42).unwrap();
    let count = t.messages.len() - 6;
    t.manifest_ready = true;
    let mut t: Transfer = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    t.restart();
    assert!(t.messages[0].contains("resume"));
    advance_to_data(&mut t, json!([[1, 2]]));
    let mut data = Vec::new();
    while t.current == 4 {
        while let Some((i, _)) = t.next_packet() {
            data.push(i);
        }
        for i in data.drain(..) {
            assert!(matches!(
                t.reply(&reply(&format!("A0000{:04x}", i - 4))),
                Reply::Next
            ));
        }
    }
    assert_eq!(t.percent(), 100);
    assert!(matches!(t.reply(&reply("C0000")), Reply::Ignored));
    assert_eq!(t.next_packet().unwrap().1, "C0000");
    // C may discover a missing durable chunk after recovery; P ranges reopen only that gap.
    assert!(matches!(t.reply(&json!({"kind":"P","themeId":t.batch_id,"fileIndex":0,"status":"resume","window":4,"receivedRanges":[[0,count-2]]})), Reply::Next));
    let missing = t.next_packet().unwrap();
    assert_eq!(missing.0, count + 3);
    assert!(t.next_packet().is_none());
    assert!(matches!(
        t.reply(&reply(&format!("A0000{:04x}", count - 1))),
        Reply::Next
    ));
    assert_eq!(t.next_packet().unwrap().1, "C0000");
    assert!(matches!(t.reply(&reply("C0000")), Reply::Next));
    let finish = t.next_packet().unwrap();
    assert!(finish.1.contains("finish"));
    assert!(matches!(
        t.reply(&json!({"kind":"T","operation":"status","status":"ready","themeId":"wrong"})),
        Reply::Ignored
    ));
    assert!(matches!(
        t.reply(&json!({"kind":"T","operation":"status","status":"ready","themeId":t.batch_id})),
        Reply::Complete
    ));
    assert!(count > 4);
    let mut fresh = Transfer::new("band".into(), vec![article()], 45).unwrap();
    fresh
        .negotiate(&json!({"version":2,"maxTextChars":256,"maxWindow":2}))
        .unwrap();
    assert!(fresh.messages.iter().all(|p| p.len() <= 256));
    assert_eq!(fresh.window, 2);
    assert!(
        fresh
            .negotiate(&json!({"version":1,"maxTextChars":256,"maxWindow":2}))
            .is_err()
    );
    assert!(Transfer::new("a".into(), vec![], 1).is_err());
    assert!(Transfer::new("a".into(), vec![article(); news::MAX_ARTICLES + 1], 1).is_err());
    let mut long = article();
    long.body = "长".repeat(400000);
    assert!(Transfer::new("a".into(), vec![long], 1).is_err());
}
#[test]
fn corona_sender_skips_foreign_themes_aborts_on_reject_and_bounds_limits() {
    let mut t = Transfer::new("band".into(), vec![article()], 7).unwrap();
    // Corona's sender caps maxTextChars at its own 18,000 and reserves 11 header chars.
    assert!(
        t.negotiate(&json!({"version":2,"maxTextChars":18001,"maxWindow":4}))
            .is_err()
    );
    t.negotiate(&json!({"version":2,"maxTextChars":256,"maxWindow":4}))
        .unwrap();
    assert_eq!(t.chunk_bytes, (256 - 11) * 13 / 16);
    assert!(t.messages.iter().all(|p| p.len() <= 256));
    assert_eq!(t.next_packet().unwrap().0, 0);
    let id = t.batch_id.clone();
    assert!(matches!(
        t.reply(&json!({"kind":"E","themeId":"other","errorCode":"write-failed"})),
        Reply::Ignored
    ));
    assert!(matches!(
        t.reply(&json!({"kind":"T","operation":"status","status":"reject","themeId":id,"errorCode":"invalid-manifest"})),
        Reply::Error(_)
    ));
    assert!(matches!(
        t.reply(&json!({"kind":"E","themeId":id,"errorCode":"write-failed"})),
        Reply::Error(_)
    ));
    let detailed = reply(&format!(
        r#"E{{"themeId":"{id}","errorCode":"write-failed","message":"保存传输进度失败：writeText (300) disk full","nativeCode":300,"operation":"writeText"}}"#
    ));
    assert!(
        matches!(t.reply(&detailed), Reply::Error(message) if message.contains("writeText (300) disk full"))
    );
    // Four attempts per outstanding packet: three retries, then pause.
    assert!(t.retry_pending() && t.retry_pending() && t.retry_pending());
    assert!(!t.retry_pending());
}

#[test]
fn half_mib_payload_is_packetized_without_truncation_and_one_extra_byte_is_rejected() {
    use pocket_news::transfer::MAX_BYTES;
    assert_eq!(MAX_BYTES, 512 * 1024);
    let mut a = article();
    let base = Transfer::new("band".into(), vec![a.clone()], 42)
        .unwrap()
        .payload
        .len();
    a.body.push_str(&"x".repeat(MAX_BYTES - base));
    let task = Transfer::new("band".into(), vec![a.clone()], 42).unwrap();
    assert_eq!(task.payload.len(), MAX_BYTES);
    let decoded: pocket_news::transfer::Batch = serde_json::from_str(&task.payload).unwrap();
    assert_eq!(decoded.articles[0].body, a.body);
    assert_eq!(
        serde_json::from_str::<Value>(&task.messages[0][1..]).unwrap()["totalBytes"],
        MAX_BYTES
    );
    assert!(task.messages.iter().all(|packet| packet.len() <= 18000));
    a.body.push('x');
    assert!(
        Transfer::new("band".into(), vec![a], 42)
            .err()
            .unwrap()
            .contains("512 KB")
    );
}

#[test]
fn only_the_final_durable_commit_uses_the_longer_acknowledgement_timeout() {
    use pocket_news::transfer::{ACK_TIMEOUT_MS, COMMIT_TIMEOUT_MS};
    let mut transfer = Transfer::new("band".into(), vec![article()], 99).unwrap();
    for current in 0..transfer.messages.len() {
        transfer.current = current;
        assert_eq!(
            transfer.acknowledgement_timeout_ms(),
            if current + 1 == transfer.messages.len() {
                COMMIT_TIMEOUT_MS
            } else {
                ACK_TIMEOUT_MS
            }
        );
    }
    assert_eq!(COMMIT_TIMEOUT_MS, 120000);
}

#[test]
fn readiness_handshake_identifies_the_current_run_and_preserves_the_batch_for_resume() {
    let mut transfer = Transfer::new("band".into(), vec![article()], 99).unwrap();
    transfer.restart();
    let first: Value = serde_json::from_str(&transfer.handshake()[1..]).unwrap();
    assert_eq!(first["requestId"], transfer.handshake_id());
    assert_eq!(first["themeId"], transfer.batch_id);
    transfer.restart();
    let second: Value = serde_json::from_str(&transfer.handshake()[1..]).unwrap();
    assert_ne!(first["requestId"], second["requestId"]);
    assert_eq!(first["themeId"], second["themeId"]);
    assert!(transfer.handshake_id().len() <= 64);
}

#[test]
fn source_pictures_are_dropped_and_batches_stay_text_only() {
    let html = r#"<p>开头内容</p><img data-src='../photo.png?a=1&amp;b=2' alt='配图'><p>中间内容</p><img src='//cdn.example.com/b.webp'><p>最后内容</p>"#;
    let rss = format!(
        "<rss><channel><item><title>图文新闻</title><link>https://example.com/news/item.html</link><description><![CDATA[{html}]]></description></item></channel></rss>"
    );
    let parsed = news::parse_rss(&rss, "News", "tech").unwrap().remove(0);
    assert_eq!(
        parsed.body,
        "开头内容

中间内容

最后内容"
    );
    let payload = serde_json::to_value(&parsed).unwrap();
    assert!(payload.get("content").is_none());
    assert!(
        !serde_json::to_string(&payload)
            .unwrap()
            .contains("photo.png")
    );
    // Redirects still resolve relative links to HTTPS without fragments.
    assert_eq!(
        news::resolve_url("http://example.com/a/b", "../c#top").as_deref(),
        Some("https://example.com/c")
    );
    assert!(news::resolve_url("https://example.com/", "data:image/png;base64,AA").is_none());
    // The watch no longer advertises pictures; text batches never depend on it.
    let mut t = Transfer::new("band".into(), vec![parsed], 88).unwrap();
    assert!(
        t.negotiate(&json!({"version":2,"maxTextChars":18000,"maxWindow":4,"features":[]}))
            .is_ok()
    );
}
