use pocket_news::{
    classification::{self, classify},
    news, sources,
    transfer::{Batch, Transfer},
};
use serde_json::{Value, json};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
fn feed(id: &str) -> usize {
    sources::feeds().iter().position(|f| f.id == id).unwrap()
}
fn cls(rows: Value) -> Vec<news::Article> {
    sources::parse_response(feed("cls"), &json!({"data":{"roll_data":rows}}).to_string()).unwrap()
}
fn infer(title: &str, body: &str) -> &'static str {
    classify(title, body, "https://www.cls.cn/detail/1", &[], &[], "")
}

#[test]
fn cls_weather_and_other_subjects_are_not_forced_into_finance() {
    let articles = cls(json!([
        {"id":1,"title":"中央气象台发布暴雨橙色预警","brief":"财联社11月14日电，明日多地将出现强降雨，气温下降。"},
        {"id":2,"title":"美股三大股指集体低开","brief":"纳斯达克指数下跌，英伟达股价跌幅扩大。"},
        {"id":3,"title":"国防部通报海军实弹演习","brief":"军舰编队开展实弹训练。"},
        {"id":4,"title":"中国女排晋级决赛","brief":"球队将争夺冠军。"},
        {"id":5,"title":"某地发生地震，消防正在救援","brief":"被困居民已转移。"},
        {"id":6,"title":"高考招生计划公布","brief":"高校公布录取分数。"},
        {"id":7,"title":"健康提示：如何预防流感","brief":"医生建议及时接种疫苗。"},
        {"id":8,"title":"新发现文物将亮相博物馆","brief":"考古成果展览。"},
        {"id":9,"title":"新片票房创新高","brief":"电影节颁奖。"},
        {"id":10,"title":"科研团队发布量子计算新成果","brief":"科学家介绍新技术。"}
    ]));
    let expected = [
        "life",
        "world",
        "military",
        "sports",
        "society",
        "education",
        "health",
        "culture",
        "entertainment",
        "tech",
    ];
    assert_eq!(articles.len(), expected.len());
    for (article, category) in articles.iter().zip(expected) {
        assert_eq!(article.category, category, "{}", article.title);
        assert_eq!(article.source, "财联社");
    }
}

#[test]
fn weather_is_available_when_only_life_and_cls_are_selected_and_push_keeps_it() {
    let selected = strings(&["life"]);
    assert_eq!(
        sources::jobs(&selected, &strings(&["cls"])),
        vec![feed("cls")]
    );
    let articles = cls(json!([
        {"id":1,"title":"天气报告：冷空气来袭，气温下降","brief":"<p>来源完整天气正文</p>","ctime":1000},
        {"id":2,"title":"上市公司财报：净利润增长","brief":"来源财经正文","ctime":1001}
    ]));
    let batch = news::collect_batch_limit(articles, &selected, 30);
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].category, "life");
    assert_eq!(batch[0].body, "来源完整天气正文");
    assert_eq!(batch[0].published_at, Some(1_000_000));
    let task = Transfer::new("band".into(), batch, 42).unwrap();
    let payload: Batch = serde_json::from_str(&task.payload).unwrap();
    assert_eq!(payload.articles[0].category, "life");
    assert_eq!(payload.articles[0].url, "https://www.cls.cn/detail/1");
    assert_eq!(payload.articles[0].source, "财联社");
    assert_eq!(payload.articles[0].body, "来源完整天气正文");
}

#[test]
fn subject_is_independent_of_selected_categories_and_provider_defaults() {
    for id in ["cls", "jin10", "legacy-finance", "legacy-tech"] {
        assert!(
            sources::jobs(
                &strings(&["life"]),
                &strings(&[&sources::feeds()[feed(id)].provider])
            )
            .contains(&feed(id)),
            "{id}"
        );
    }
    let articles = cls(json!([{"id":1,"title":"天气预报：明日有雨"}]));
    assert!(news::collect_batch_limit(articles.clone(), &strings(&["finance"]), 30).is_empty());
    assert_eq!(
        news::collect_batch_limit(articles, &strings(&["life"]), 30)[0].category,
        "life"
    );
    for source in ["财联社", "IT之家", "华尔街见闻", "腾讯新闻"] {
        let parsed = news::parse_rss("<rss><channel><item><title>天气预报：冷空气来袭</title><link>https://example.com/1</link></item></channel></rss>", source, "").unwrap();
        assert_eq!(parsed[0].category, "life", "{source}");
    }
}

#[test]
fn clear_events_override_broad_metadata_without_leaking_between_rows() {
    let articles = cls(json!([
        {"id":1,"title":"科技公司财报公布","category":"科技频道"},
        {"id":2,"title":"科技公司财报公布","channel":{"name":"财经"}},
        {"id":3,"title":"天气报告：降温","category":12345},
        {"id":4,"title":"天气报告：降温","category":"综合要闻"},
        {"id":5,"title":"内容介绍","category":[{"id":1,"name":"体育"}]}
    ]));
    let expected = ["finance", "finance", "life", "life", "sports"];
    for (a, c) in articles.iter().zip(expected) {
        assert_eq!(a.category, c, "{}", a.url);
    }
}

#[test]
fn rss_reads_each_items_categories_and_ignores_feed_level_categories() {
    let xml = r#"<rss><channel><category>财经</category>
        <item><title>天气报告</title><link>https://example.com/1</link><category><![CDATA[健康]]></category></item>
        <item><title>天气报告</title><link>https://example.com/2</link><category>综合</category></item>
        <item><title>今日消息</title><link>https://example.com/3</link><category>文化新闻</category></item>
        <item><title>科技公司公布财报</title><link>https://example.com/4</link><category>technology</category></item>
        </channel></rss>"#;
    let parsed = news::parse_rss(xml, "测试媒体", "finance").unwrap();
    assert_eq!(
        parsed
            .iter()
            .map(|a| a.category.as_str())
            .collect::<Vec<_>>(),
        ["life", "life", "culture", "finance"]
    );
}

#[test]
fn atom_reads_term_and_label_in_empty_and_nonempty_categories() {
    let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><category term="finance"/>
        <entry><title>今日消息</title><link href="https://example.com/1"/><category term="education"/></entry>
        <entry><title>今日消息</title><link href="https://example.com/2"/><category term="123" label="健康"></category></entry>
        <entry><title>天气预报：气温下降</title><link href="https://example.com/3"/><category term="global-channel"/></entry>
        </feed>"#;
    let parsed = news::parse_rss(xml, "测试媒体", "").unwrap();
    assert_eq!(
        parsed
            .iter()
            .map(|a| a.category.as_str())
            .collect::<Vec<_>>(),
        ["education", "health", "life"]
    );
}

#[test]
fn legacy_wallstreet_broadcast_channels_are_not_specific_article_subjects() {
    let text = json!({"data":{"items":[
        {"title":"中央气象台发布寒潮预警","uri":"https://wallstreetcn.com/livenews/1","channels":["global-channel","financing-channel"]},
        {"title":"英伟达股价跌幅扩大","uri":"https://wallstreetcn.com/livenews/2","channels":["global-channel"]}
    ]}}).to_string();
    let parsed = sources::parse_response(feed("legacy-finance"), &text).unwrap();
    assert_eq!(parsed[0].category, "life");
    assert_eq!(parsed[1].category, "world");
}

#[test]
fn trusted_article_urls_supply_columns_but_search_terms_and_publisher_names_do_not() {
    for (url, expected) in [
        ("https://www.chinanews.com.cn/ty/2026/11/14/1.shtml", "life"),
        ("https://www.zaochenbao.com/realtime/world/1.html", "life"),
        ("https://www.chinanews.com.cn/edu/1.shtml", "life"),
        ("https://finance.ifeng.com/c/1", "life"),
        ("https://news.qq.com/a/1?category=finance", "life"),
        ("https://www.cls.cn/detail/1", "life"),
        ("https://unknown.example/finance/1", "life"),
        ("https://cls.cn.attacker.example/finance/1", "life"),
    ] {
        assert_eq!(
            classify("天气报告：明日降温", "", url, &[], &[], ""),
            expected,
            "{url}"
        );
    }
}

#[test]
fn cross_subject_headlines_use_the_main_event_not_a_single_incidental_word() {
    for (title, expected) in [
        ("寒潮影响小麦供给，农产品期货涨停", "finance"),
        ("暴雨预警：明日气温下降", "life"),
        ("暴雨引发山洪，多人被困，消防紧急救援", "society"),
        ("药企公布财报，净利润大增", "finance"),
        ("医院发布健康提示：如何预防流感", "health"),
        ("体育用品公司IPO，营收增长", "finance"),
        ("美国篮球联赛决赛，中国球员夺冠", "world"),
        ("大学科研团队发布量子计算新成果", "tech"),
        ("科技公司股价大涨，市值创新高", "finance"),
        ("外交部就国际峰会答记者问", "china"),
        ("美国总统就外交会谈发表声明", "world"),
        ("国务院出台义务教育改革政策", "education"),
    ] {
        assert_eq!(infer(title, ""), expected, "{title}");
    }
    assert_eq!(
        infer("美国总统就外交会谈发表声明", "曾到访当地一家银行和医院。"),
        "world"
    );
}

#[test]
fn repeated_body_words_do_not_overrule_a_clear_headline() {
    assert_eq!(
        infer(
            "健康提示：如何预防流感",
            &"财联社11月14日电，市场、银行、股票、公司、基金。".repeat(500)
        ),
        "health"
    );
    assert_eq!(
        infer("今日快讯", "证券时报11月14日消息，中央气象台发布暴雨预警。"),
        "life"
    );
    assert_eq!(
        infer("今日快讯", "财联社11月14日电，天气预报提示大风降温。"),
        "life"
    );
    assert_eq!(infer("今日快讯", "股价上涨"), "finance");
}

#[test]
fn topic_tags_are_lower_priority_than_channels_and_cannot_hijack_headlines() {
    let articles = cls(json!([
        {"id":1,"title":"英伟达股价跌幅扩大至4.2%","subjects":[{"subject_name":"半导体芯片"},{"subject_name":"环球市场情报"}]},
        {"id":2,"title":"天气报告：明日降温","subjects":[{"subject_name":"美股动态"}]},
        {"id":3,"title":"今日消息","category":"健康","tags":[{"name":"财经"}]}
    ]));
    assert_eq!(articles[0].category, "world");
    assert_eq!(articles[1].category, "life");
    assert_eq!(articles[2].category, "health");
    assert!(classification::metadata(&json!({"id":1,"content":"财经","count":1})).is_empty());
    assert!(classification::metadata(&json!([1, 2, 3])).is_empty());
}

#[test]
fn a_plain_duplicate_does_not_drop_the_trending_copy_membership() {
    let mut plain = cls(json!([{"id": 1, "title": "女排夺冠"}]));
    let mut trending = plain.clone();
    plain[0].hot = false;
    plain[0].published_at = Some(2);
    trending[0].hot = true;
    trending[0].published_at = Some(1);
    plain.append(&mut trending);
    let chosen = news::collect_batch_limit(plain, &strings(&["sports", "hot"]), 30);
    assert_eq!(chosen.len(), 1);
    assert!(chosen[0].matches_category("hot") && chosen[0].matches_category("sports"));
}

#[test]
fn insufficient_information_keeps_articles_in_an_existing_channel() {
    let articles = cls(json!([{"id":1,"title":"今日消息"},{"id":2,"title":"更多消息"}]));
    assert_eq!(articles.len(), 2);
    assert!(
        articles
            .iter()
            .all(|a| news::CATEGORIES.iter().any(|(id, _)| *id == a.category))
    );
    assert_eq!(
        classify(
            "今日消息",
            "",
            "https://example.com/1",
            &[],
            &[],
            "education"
        ),
        "education"
    );
    assert!(!classification::has_reliable_subject(
        "chair design",
        "",
        "https://example.com/1",
        infer("chair design", "")
    ));
}

#[test]
fn captured_headline_regressions_cover_non_financial_publishers_and_word_boundaries() {
    for (title, body, expected) in [
        ("费城半导体指数跌2%，报9824.51点", "", "world"),
        ("【金牌纪要库】模型厂商布局端侧AI", "", "tech"),
        ("Coursera将裁减约10%的全球员工", "将产生遣散费用。", "world"),
        ("埃森哲推出服务，助力企业构建AI数据基础", "", "world"),
        ("马来西亚交通部长宣布其辞职决定立即生效", "", "world"),
        ("伊朗否认针对法国的网络攻击，伊朗外交部回应", "", "world"),
        ("美国两年期国债收益率上涨", "", "world"),
        ("国内首只5年期地方债ETF规模突破百亿", "", "finance"),
        ("LME三个月期铝下跌", "", "world"),
        ("诺和诺德新药注射液上市申请获药监局受理", "", "health"),
        ("前三季度保险赔款与给付支出同比增长", "", "finance"),
        ("显卡终于降价了", "英伟达股价近日出现波动。", "tech"),
        (
            "DeepSeek和B站也有iPhone版本了，全球适配应用超10万",
            "",
            "tech",
        ),
        (
            "赢了宅斗却没有走出大宅，古装剧需要什么样的大女主",
            "",
            "entertainment",
        ),
        (
            "独立双系统：法式门双变频大冰箱直降",
            "健康生活，从一台冰箱开始。",
            "life",
        ),
        ("鸿蒙HarmonyOS补丁包推送", "", "tech"),
        ("首次发行H股并在香港联交所上市", "", "finance"),
    ] {
        assert_eq!(infer(title, body), expected, "{title}");
    }
    let text = cls(
        json!([{"id":1,"title":"埃森哲推出服务，助力企业构建AI数据基础", "subjects":[{"subject_name":"美股动态"}]}]),
    );
    assert_eq!(text[0].category, "world");
}

#[test]
fn second_round_captured_headlines_and_headline_tie_breaks() {
    for (title, body, expected) in [
        ("某区原副书记严重违纪违法被开除党籍和公职", "", "china"),
        ("ADP：过去四周美国私营部门就业平均每周增加", "", "world"),
        ("BioNTech购买FDA优先审评券", "", "world"),
        ("美最高法院放行得州边境墙项目", "", "world"),
        ("湖南28岁男子泰国失联已逾4个月", "", "society"),
        ("两男子靠守号喜获3000万", "体育彩票开奖。", "society"),
        ("2026中足联颁奖典礼", "", "sports"),
        ("EDG官宣选手离队", "", "sports"),
        ("波切蒂诺 曼联", "", "sports"),
        ("iPhoneDuo不用抢了", "", "tech"),
        (
            "iPhone 18 Pro 京东焕新，旧机估值满 1000 补贴 1200 元",
            "",
            "tech",
        ),
        (
            "iQOO Z11 Turbo 国补直降，骁龙与 2 亿主摄加持",
            "冰箱、洗衣机同步促销。",
            "tech",
        ),
        ("美国造车新势力获最大股东贷款额度", "", "world"),
        ("小米米家学习大路灯新低", "", "life"),
        ("领克900L首发亮相", "", "tech"),
        // One headline clue against a capped body: the headline decides, not the TOPICS order.
        (
            "3000架无人机点亮夜空致敬空军",
            "博物馆文化艺术展演，传统文化与历史文明。",
            "military",
        ),
    ] {
        assert_eq!(infer(title, body), expected, "{title}");
    }
}

#[test]
fn event_frames_distinguish_business_products_accidents_and_public_services() {
    let cases = [
        ("苹果公布财报，净利润同比增长20%", "world"),
        ("电影公司公布季度财报，营收增长", "finance"),
        ("苹果发布新款手机，股价随后上涨", "world"),
        ("博物馆发生火灾，消防紧急救援", "society"),
        ("演员因涉嫌诈骗被警方刑拘", "society"),
        ("新电影上映，剧中警察追查诈骗案件", "entertainment"),
        ("高校实验室研发新款量子芯片", "tech"),
        ("科学家发现癌症治疗新靶点", "health"),
        ("教育部通报校园欺凌事件，警方已介入调查", "society"),
        ("医院就患者死亡事件发布通报", "society"),
        ("医生提醒夏季高温如何预防中暑", "health"),
        ("美国发布暴雨预警，多地气温下降", "world"),
        ("台风致港口停运，原油期货上涨", "finance"),
        ("军舰成功发射新型导弹", "military"),
        ("国防大学公布研究生招生计划", "education"),
        ("美国总统宣布对外军事行动", "world"),
        ("国乒公布世乒赛参赛名单", "sports"),
        ("多地上调最低工资标准", "society"),
        ("央行下调存款准备金率", "finance"),
        ("以军空袭加沙致多人死亡", "world"),
        ("某地地震引发海啸预警", "society"),
        ("博物馆发布新发现文物展览计划", "culture"),
        ("小米发布新款智能手机", "tech"),
        ("药企新药获批上市", "health"),
        ("旅游攻略：春节出游如何安排行程", "life"),
    ];
    let failures: Vec<_> = cases
        .into_iter()
        .filter_map(|(title, expected)| {
            let actual = infer(title, "");
            (actual != expected).then(|| format!("{title}: {actual} != {expected}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Metadata is still authoritative even for a headline whose subject is ambiguous.
    assert_eq!(
        classify(
            "今日发布",
            "",
            "https://example.com/1",
            &strings(&["医药"]),
            &[],
            ""
        ),
        "health"
    );
}

#[test]
fn broad_geographic_sections_yield_to_specific_events_and_nested_columns_use_the_leaf() {
    for (title, column, expected) in [
        ("中国女排晋级决赛", "国内", "sports"),
        ("美国发布暴雨预警", "国际", "world"),
        ("美国科技公司公布财报", "国际", "world"),
        ("今日消息", "国际", "world"),
        ("今日消息", "国内", "china"),
        ("今日消息", "国内/科技/人工智能", "tech"),
        ("今日消息", "国际>体育", "sports"),
    ] {
        assert_eq!(
            classify(
                title,
                "",
                "https://example.com/a",
                &strings(&[column]),
                &[],
                ""
            ),
            expected,
            "{title} / {column}"
        );
    }
}

#[test]
fn reported_misclassifications_and_adjacent_product_economic_events() {
    for (title, body, expected) in [
        (
            "利用 Claude 辅助编程：爱好者将 Seeed Studio“电子墨水屏便利贴”改造为苹果 Apple TV 遥控器",
            "开发者开源了项目，使用电子墨水屏控制智能设备。",
            "tech",
        ),
        (
            "乘联分会崔东树：十年间新车平均车长增至 4.72 米，SUV 以 56% 份额强势称王",
            "乘联分会发布中国乘用车市场的行业统计数据。",
            "finance",
        ),
        (
            "倍思推出 60W 单 USB-C 充电头：支持 AVS 协议，89 元",
            "新款充电器支持快充，采用可折叠插脚并提供多种输出功率。",
            "tech",
        ),
        (
            "行业协会：智能手机市场销量同比增长12%，高端产品占比提升",
            "协会发布新一季度市场统计数据。",
            "finance",
        ),
        (
            "苹果发布新款手机，股价随后上涨",
            "手机采用新一代处理器。",
            "world",
        ),
        (
            "利用AI将国漫角色改成情色形象，7名画师获刑",
            "法院宣判了相关案件。",
            "society",
        ),
        (
            "游戏工作室办公楼遭枪击，警方逮捕嫌疑人",
            "警方已封锁现场。",
            "society",
        ),
        ("韩国考生利用AI作弊", "考试组织方展开调查。", "world"),
        (
            "公益项目利用AI创新教育，覆盖2000所乡村学校",
            "项目为学生提供学习资源。",
            "education",
        ),
        (
            "特斯拉Model Y续航提升8.5%",
            "新款车型的续航得到改善。",
            "world",
        ),
        (
            "欧盟拟推迟对进口电动汽车征收10%关税",
            "新政策涉及汽车进口关税。",
            "world",
        ),
        (
            "中文学习机HSK专业版首秀",
            "产品支持中文学习和语音交互。",
            "tech",
        ),
        (
            "社交VR平台宣布重返Steam",
            "平台为用户提供虚拟现实交互功能。",
            "tech",
        ),
        ("赵丽颖晒照引网友点赞", "", "entertainment"),
        ("梅西和C罗的球迷谁更多", "", "sports"),
        (
            "国务院印发地方工作条例",
            "文件要求各地区认真贯彻执行。",
            "china",
        ),
        (
            "新药注射液上市申请获药监局受理",
            "药品将用于患者的疾病治疗。",
            "health",
        ),
    ] {
        let url = "https://www.ithome.com/1/027/414.htm";
        let category = classify(title, body, url, &[], &[], "");
        assert_eq!(category, expected, "{title}");
        assert!(
            classification::has_reliable_subject(title, body, url, category),
            "{title}"
        );
    }
}

#[test]
fn five_reported_articles_follow_confirmed_geographic_priority() {
    let cases = [
        (
            "外媒试玩《GTA 6》：主角感情需要经营，抢劫也要讲策略",
            "英国媒体介绍海外游戏的试玩体验和故事玩法。",
            "world",
        ),
        (
            "黄仁勋卸任后，英伟达新任 CEO 面临重建员工信任难题",
            "报道讨论这家美国公司的管理层变动与员工关系。",
            "world",
        ),
        (
            "微软否认与全美航班停飞事件有关",
            "美国企业回应美国联邦航空管理局的停飞事件。",
            "world",
        ),
        (
            "*ST闻泰：企业法庭就安世境外追加临时措施申请作出裁定，公司拟上诉",
            "中国上市公司就境外司法程序披露公告，说明法院裁定及上诉安排。",
            "society",
        ),
        (
            "美国联邦航空管理局解除地面停飞令，航班获准恢复起降",
            "美国航空主管机关解除了当地航班起降限制。",
            "world",
        ),
    ];
    for (title, body, expected) in cases {
        assert_eq!(infer(title, body), expected, "{title}");
    }
}
