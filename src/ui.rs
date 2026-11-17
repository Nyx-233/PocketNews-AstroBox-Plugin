use crate::{
    app::{self, State},
    astrobox::psys_host_v4::ui::{
        self, Element as El, ElementType as Ty, Event, FlexDirection as Dir,
    },
    news::{self, Article},
    selection::Filter,
};

const CONTROL_RADIUS: u32 = 12;
// One-tap counts beside the slider; the slider and number input still allow any value.
const COUNT_PRESETS: [usize; 4] = [10, 30, 50, 100];

fn paragraph(value: &str, size: u32) -> El {
    El::new(Ty::P, Some(value))
        .size(size)
        .margin(0)
        .min_width(0)
}
fn minor(value: &str) -> El {
    paragraph(value, 12).opacity(0.65)
}
fn group() -> El {
    El::new(Ty::Div, None)
        .flex()
        .flex_direction(Dir::Column)
        .width_full()
        .min_width(0)
        .gap(8)
}
fn line() -> El {
    El::new(Ty::Div, None)
        .flex()
        .flex_direction(Dir::Row)
        .align_center()
        .width_full()
        .min_width(0)
        .gap(8)
}
fn rule() -> El {
    El::new(Ty::Separator, None).width_full()
}
fn card(key: &str) -> El {
    El::new(Ty::Card, None)
        .radius(CONTROL_RADIUS)
        .prop("key", key)
        .width_full()
        .padding(16)
}
fn plugin_styles() -> El {
    // Inline SVG styles are supported by the host; every selector is scoped to this plugin.
    // The range keeps the browser's own track and fill, so the filled part moves with the
    // thumb in the same frame instead of waiting for a plugin re-render.
    let style = r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"><style>
input.pocket-news-range { width:100%; margin:0; accent-color:var(--accent-9); cursor:pointer; }
input.pocket-news-range:enabled:hover { accent-color:var(--accent-10); }
input.pocket-news-range:disabled { opacity:0.5; cursor:not-allowed; }
input.pocket-news-range:focus-visible { outline:2px solid var(--accent-8); outline-offset:2px; }
.pocket-news-wrap { white-space:normal !important; overflow-wrap:anywhere; word-break:break-word; height:auto !important; min-height:36px; max-width:100%; padding-top:8px; padding-bottom:8px; line-height:1.35; text-align:left; justify-content:flex-start !important; }
.pocket-news-fold { min-width:36px; }
.pocket-news-fold::before { content:""; flex-shrink:0; width:8px; height:8px; border-style:none solid solid none; border-width:2px; }
.pocket-news-fold-closed::before { transform:translateY(-2px) rotate(45deg); }
.pocket-news-fold-open::before { transform:translateY(2px) rotate(-135deg); }
</style></svg>"#;
    El::new(Ty::Svg, Some(style))
        .prop("key", "plugin-styles")
        .width(0)
        .height(0)
}
fn action(value: &str, id: &str, disabled: bool) -> El {
    let element = El::new(Ty::Button, Some(value))
        .radius(CONTROL_RADIUS)
        .prop("class", "whitespace-nowrap")
        .flex_shrink(0.0)
        .on(Event::Click, id);
    if disabled {
        element
            .prop("variant", "soft")
            .bg("var(--gray-a3)")
            .text_color("var(--gray-11)")
            .disabled()
    } else {
        element
    }
}
fn soft(value: &str, id: &str, disabled: bool) -> El {
    action(value, id, disabled).prop("variant", "soft")
}
// Folding uses chevrons drawn by plugin_styles(): an up chevron folds an open section,
// a down chevron opens a folded one.
fn fold(open: bool, id: &str, busy: bool) -> El {
    let shape = if open {
        "pocket-news-fold-open"
    } else {
        "pocket-news-fold-closed"
    };
    soft("", id, busy)
        .prop(
            "class",
            &format!("whitespace-nowrap pocket-news-fold {shape}"),
        )
        .prop("aria-label", if open { "收起" } else { "展开" })
        .prop("title", if open { "收起" } else { "展开" })
}
fn section_title(value: &str) -> El {
    paragraph(value, 15).flex_grow(1.0)
}
fn subsection_title(value: &str) -> El {
    paragraph(value, 14).flex_grow(1.0)
}
// Highlight only the current choice; everything else uses the neutral soft style.
fn toggle(name: &str, id: &str, current: bool, busy: bool) -> El {
    let text = if current {
        format!("✓ {name}")
    } else {
        name.to_string()
    };
    let button = action(&text, id, busy);
    if current && !busy {
        button
    } else {
        button
            .prop("variant", "soft")
            .bg("var(--gray-a3)")
            .text_color("var(--gray-12)")
    }
}
fn clock(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp as i64)
        .map(|value| {
            (value + chrono::Duration::hours(8))
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}
fn choice(name: &str, id: &str, picked: bool, busy: bool) -> El {
    toggle(name, id, picked, busy)
        .width_full()
        .min_height(36)
        .prop("data-state", if picked { "on" } else { "off" })
        .border(
            if picked && !busy { 2 } else { 1 },
            if picked && !busy {
                "var(--accent-9)"
            } else {
                "var(--gray-a6)"
            },
        )
        .radius(CONTROL_RADIUS)
}
fn choice_grid(min: u32) -> El {
    El::new(Ty::Grid, None)
        .grid_template_columns(&format!("repeat(auto-fit, minmax({min}px, 1fr))"))
        .gap(8)
        .width_full()
}
fn chosen_channels(state: &State) -> String {
    let names = news::CATEGORIES
        .iter()
        .filter(|(id, _)| state.saved.preferences.iter().any(|value| value == id))
        .map(|(_, name)| *name)
        .collect::<Vec<_>>();
    match names.len() {
        0 => "未选频道".into(),
        1..=3 => names.join("、"),
        n => format!("{}等 {n} 个频道", names[..2].join("、")),
    }
}
fn sources(state: &State, busy: bool) -> El {
    let selected = &state.saved.source_preferences;
    let total = crate::sources::providers().len();
    let open = state.sources_open;
    let mut section = group().prop("key", "news-sources").child(
        line()
            .child(subsection_title("新闻来源"))
            .child(minor(&format!("已选 {} / {total}", selected.len())))
            .child(fold(open, "toggle-sources", busy)),
    );
    if open {
        let mut grid = choice_grid(120);
        for provider in crate::sources::providers() {
            grid = grid.child(choice(
                &provider.name,
                &format!("source:{}", provider.id),
                selected.contains(&provider.id),
                busy,
            ));
        }
        section = section
            .child(
                line()
                    .child(minor("自动补抓公开原文；无法补齐会说明原因").flex_grow(1.0))
                    .child(soft("全选", "sources-all", busy || selected.len() == total))
                    .child(soft(
                        "取消全选",
                        "sources-clear",
                        busy || selected.is_empty(),
                    )),
            )
            .child(grid);
    }
    section
}
fn channels(state: &State, busy: bool) -> El {
    let chosen = &state.saved.preferences;
    let total = news::CATEGORIES.len();
    let mut section = group().prop("key", "news-channels").child(
        line()
            .child(subsection_title("新闻频道"))
            .child(minor(&format!("已选 {} / {total}", chosen.len())))
            .child(fold(state.channels_open, "toggle-channels", busy)),
    );
    if state.channels_open {
        let mut grid = choice_grid(88);
        for (id, name) in news::CATEGORIES {
            let picked = chosen.iter().any(|value| value == id);
            grid = grid.child(choice(name, &format!("category:{id}"), picked, busy));
        }
        section = section
            .child(
                line()
                    .child(minor("点击频道可多选").flex_grow(1.0))
                    .child(soft(
                        "全选",
                        "categories-all",
                        busy || chosen.len() == total,
                    ))
                    .child(soft(
                        "取消全选",
                        "categories-clear",
                        busy || chosen.is_empty(),
                    )),
            )
            .child(grid);
    }
    section
}
fn article_count(state: &State, busy: bool) -> El {
    let input = El::new(Ty::Input, None)
        .prop("type", "number")
        .prop("default-value", &state.saved.article_count.to_string())
        .prop("key", &format!("count-input-{}", state.saved.article_count))
        .prop("min", "1")
        .prop("max", "100")
        .prop("step", "1")
        .prop("input-mode", "numeric")
        .prop("aria-label", "获取篇数")
        .width(76)
        .on(Event::Input, "article-count-input")
        .on(Event::Change, "article-count-input")
        .on(Event::Blur, "article-count-input");
    // The host Slider only emits onValueCommit. A native range emits live input events.
    let slider = El::new(Ty::Input, None)
        .without_default_styles()
        .prop("type", "range")
        .prop("min", "1")
        .prop("max", "100")
        .prop("step", "1")
        .prop("default-value", &state.saved.article_count.to_string())
        .prop("key", &format!("count-slider-{}", state.count_revision))
        .prop("class", "pocket-news-range cursor-pointer")
        .prop("aria-label", "获取篇数，1至100篇")
        .width_full()
        .height(24)
        .margin(0)
        .on(Event::Input, "article-count")
        .on(Event::PointerUp, "article-count")
        .on(Event::KeyUp, "article-count")
        .on(Event::Blur, "article-count");
    let mut presets = line().gap(6);
    for count in COUNT_PRESETS {
        presets = presets.child(toggle(
            &count.to_string(),
            &format!("count:{count}"),
            state.saved.article_count == count,
            busy,
        ));
    }
    let mut section = group().prop("key", "article-count");
    section = section
        .child(
            line()
                .child(subsection_title("获取篇数"))
                .child(if busy { input.disabled() } else { input })
                .child(minor("篇")),
        )
        .child(if busy { slider.disabled() } else { slider })
        .child(presets);
    section
}
// Sources, channels and count live in one card that folds to a summary once news is loaded.
fn settings(state: &State, busy: bool) -> El {
    let summary = format!(
        "{} 个来源 · {} · {} 篇",
        state.saved.source_preferences.len(),
        chosen_channels(state),
        state.saved.article_count
    );
    let mut section = group().prop("key", "collection-body").gap(12).child(
        line().child(section_title("获取设置")).child(fold(
            state.saved.settings_open,
            "toggle-preferences",
            busy,
        )),
    );
    if state.saved.settings_open {
        section = section
            .child(sources(state, busy))
            .child(rule())
            .child(channels(state, busy))
            .child(rule())
            .child(article_count(state, busy));
    } else {
        section = section.child(minor(&summary));
    }
    if let Some(problem) = app::fetch_problem(state) {
        section = section.child(minor(problem).text_color("var(--red-11)"));
    }
    card("collection-settings").child(section)
}
fn devices(state: &State, busy: bool, active: bool) -> El {
    let mut section = group().child(line().child(section_title("推送手环")).child(soft(
        if state.refreshing_devices {
            "刷新中"
        } else {
            "刷新"
        },
        "refresh-devices",
        active || state.refreshing_devices,
    )));
    if state.devices.is_empty() {
        section = section.child(minor(if state.checked_at == 0 {
            "正在检查连接"
        } else {
            "未检测到手环，推送时会自动重新检查"
        }));
    } else {
        // One band per row; long names wrap inside the button on narrow screens.
        let mut list = group().prop("key", "device-list");
        for (name, address) in &state.devices {
            list = list.child(
                choice(
                    &device_label(name, address),
                    &format!("device:{address}"),
                    state.target == *address,
                    busy || state.refreshing_devices,
                )
                .prop("class", "pocket-news-wrap"),
            );
        }
        section = section.child(list);
    }
    card("devices").child(
        section.child(
            line()
                .child(minor("手环回收站中的新闻七天后自动清理").flex_grow(1.0))
                .child(soft(
                    "清空回收站",
                    "clear-trash",
                    busy || state.clearing_trash.is_some() || state.target.is_empty(),
                )),
        ),
    )
}
fn article(value: &Article, picked: bool, busy: bool) -> El {
    let category = news::CATEGORIES
        .iter()
        .find(|(id, _)| *id == value.category)
        .map(|(_, name)| *name)
        .unwrap_or("");
    let summary = if value.summary.chars().count() > 60 {
        format!("{}…", news::truncate(&value.summary, 60))
    } else {
        value.summary.clone()
    };
    let date = value
        .published_at
        .map(news::publication_date)
        .unwrap_or_else(|| "日期未知".into());
    // The card and source action are DOM siblings, so source clicks cannot toggle selection.
    let card = action("", &format!("article:{}", value.id), busy)
        .without_default_styles()
        .prop("key", "selection-surface")
        .prop("class", "text-left cursor-pointer")
        .flex()
        .flex_direction(Dir::Column)
        .align_start()
        .width_full()
        .min_width(0)
        .height_full()
        .padding(16)
        .padding_bottom(54)
        .gap(8)
        .radius(CONTROL_RADIUS)
        .text_color("inherit")
        .bg(if picked && !busy {
            "var(--accent-a3)"
        } else {
            "var(--gray-a2)"
        })
        .border(
            2,
            if picked && !busy {
                "var(--accent-9)"
            } else {
                "var(--gray-a5)"
            },
        )
        .transition("background-color 160ms ease, border-color 160ms ease")
        .child(
            line()
                .align_start()
                .child(paragraph(&value.title, 16).flex_grow(1.0))
                .child(
                    paragraph(if picked { "✓" } else { "○" }, 20)
                        .text_color(if picked && !busy {
                            "var(--accent-11)"
                        } else {
                            "var(--gray-9)"
                        })
                        .flex_shrink(0.0),
                ),
        )
        .child(paragraph(&summary, 13).opacity(0.75))
        .child(minor(&format!("{category} · {date} · {}", value.source)));
    group()
        .prop("key", &format!("news-{}", value.id))
        .gap(0)
        .relative()
        .child(card)
        .child(
            soft("查看原文 ↗", &format!("open:{}", value.id), busy)
                .prop("key", "source-link")
                .bg(if picked && !busy {
                    "var(--accent-a4)"
                } else {
                    "var(--gray-a3)"
                })
                .text_color(if picked && !busy {
                    "var(--accent-11)"
                } else {
                    "var(--gray-11)"
                })
                .absolute()
                .right(12)
                .bottom(12)
                .z_index(1),
        )
}
fn articles(state: &State, busy: bool) -> El {
    let visible = state.filter.visible(&state.saved.articles, &state.selected);
    let mut heading = line().child(section_title("新闻列表"));
    if state.saved.fetched_at != 0 {
        heading = heading.child(minor(&format!("更新 {}", clock(state.saved.fetched_at))));
    }
    if !state.saved.articles.is_empty() {
        heading = heading.child(fold(state.saved.articles_open, "toggle-articles", busy));
    }
    let section = group().prop("key", "articles").child(heading);
    if state.saved.articles.is_empty() {
        return card("article-section").child(section.child(minor(if state.collecting {
            "正在获取"
        } else {
            "设置好来源和频道后，点击底部“获取新闻”"
        })));
    }
    let all_picked = !visible.is_empty() && visible.iter().all(|a| state.selected.contains(&a.id));
    // Bulk selection stays reachable while the list is folded.
    let mut section = section.child(
        line()
            .child(
                minor(&format!(
                    "已选 {} / {} 篇",
                    state.selected.len(),
                    state.saved.articles.len()
                ))
                .flex_grow(1.0),
            )
            .child(soft(
                if all_picked { "取消全选" } else { "全选" },
                "select-all",
                busy || visible.is_empty(),
            ))
            .child(soft("反选", "invert-selection", busy || visible.is_empty())),
    );
    if !state.saved.articles_open {
        return card("article-section").child(section);
    }
    let mut filters = El::new(Ty::Grid, None)
        .grid_template_columns("repeat(auto-fit, minmax(88px, 1fr))")
        .gap(6)
        .width_full()
        .child(toggle(
            "全部",
            "filter:all",
            state.filter == Filter::All,
            busy,
        ))
        .child(toggle(
            "已选",
            "filter:selected",
            state.filter == Filter::Selected,
            busy,
        ));
    for (id, name) in news::CATEGORIES {
        if state.saved.articles.iter().any(|a| a.matches_category(id)) {
            filters = filters.child(toggle(
                name,
                &format!("filter:{id}"),
                state.filter == Filter::Category(id.into()),
                busy,
            ));
        }
    }
    section = section.child(filters);
    if visible.is_empty() {
        return card("article-section").child(section.child(minor("当前筛选下没有新闻")));
    }
    let mut list = El::new(Ty::Grid, None)
        .prop("key", "article-list")
        .width_full()
        .min_width(0)
        .gap(12)
        .grid_template_columns("repeat(auto-fit, minmax(min(100%, 340px), 1fr))");
    for value in visible {
        let picked = state.selected.contains(&value.id);
        list = list.child(article(value, picked, busy));
    }
    card("article-section").child(section.child(list))
}
fn errors(state: &State) -> El {
    let mut section = group()
        .prop("key", "source-errors")
        .child(minor(&format!("{} 条获取提示", state.errors.len())));
    for error in &state.errors {
        section = section.child(minor(error));
    }
    section
}
// Bands of the same model share a name, so add the last four address digits
// like the Bluetooth name does ("Xiaomi Smart Band 10 A1B2").
fn device_label(name: &str, address: &str) -> String {
    let digits: Vec<char> = address
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let suffix: String = digits[digits.len().saturating_sub(4)..].iter().collect();
    let name = name.trim();
    if suffix.is_empty() || name.to_ascii_uppercase().ends_with(&suffix) {
        name.into()
    } else if name.is_empty() {
        suffix
    } else {
        format!("{name} {suffix}")
    }
}
fn target_name(state: &State) -> Option<String> {
    state
        .devices
        .iter()
        .find(|(_, address)| *address == state.target)
        .map(|(name, address)| device_label(name, address))
}
// The two main steps stay pinned: fetch news, then push the selection.
fn primary_actions(state: &State, busy: bool) -> El {
    let fetch = action(
        if state.saved.articles.is_empty() {
            "获取新闻"
        } else {
            "重新获取"
        },
        "fetch",
        // Stays tappable without sources or channels so a tap explains what is missing.
        busy,
    )
    .prop(
        "variant",
        if state.saved.articles.is_empty() {
            "solid"
        } else {
            "soft"
        },
    )
    .min_height(40);
    let push = action(
        &format!("推送 {} 篇", state.selected.len()),
        "push",
        busy || state.refreshing_devices || state.selected.is_empty(),
    )
    .min_height(40)
    .min_width(120);
    line().gap(8).child(fetch).child(push)
}
fn footer(state: &State, busy: bool, active: bool) -> El {
    let mut message = group().gap(6).child(minor(&state.status));
    let controls = if let Some(transfer) = &state.transfer {
        let percent = transfer.percent();
        message = message
            .child(
                El::new(Ty::Progress, None)
                    .prop("value", &percent.to_string())
                    .prop("max", "100")
                    .width_full(),
            )
            .child(minor(&format!(
                "{percent}% · {}",
                if transfer.paused {
                    "已暂停"
                } else if percent == 100 {
                    "等待手环保存"
                } else {
                    "正在传输"
                }
            )));
        line()
            .child(action(
                if transfer.paused { "继续" } else { "暂停" },
                if transfer.paused { "resume" } else { "pause" },
                if transfer.paused {
                    active
                } else {
                    state.preparing
                },
            ))
            .child(soft("放弃本批", "cancel", false))
    } else if state.collecting {
        let completed = state.collection.as_ref().map_or(0, |c| c.completed);
        let total = state.collecting_source_total.max(1);
        let percent = completed.saturating_mul(100) / total;
        message = message.child(
            El::new(Ty::Progress, None)
                .prop("key", "collection-progress")
                .prop("value", &percent.to_string())
                .prop("max", "100")
                .width_full(),
        );
        if let Some(collection) = &state.collection {
            let sources = collection.active_summary();
            message = message
                .child(minor(&if sources.is_empty() {
                    "正在准备请求".into()
                } else {
                    format!("正在请求：{sources}")
                }))
                .child(minor(&format!(
                    "{percent}% · 已用 {} 秒{}",
                    collection.elapsed_seconds(),
                    if collection.failed > 0 {
                        format!(" · {} 个来源失败", collection.failed)
                    } else {
                        String::new()
                    }
                )));
        }
        soft("取消", "cancel", false)
    } else if state.preparing {
        soft("取消", "cancel", false)
    } else {
        message = message.child(minor(&if !state.saved.articles.is_empty()
            && state.selected.is_empty()
        {
            "点击新闻卡片选择要推送的内容".into()
        } else if let Some(name) = target_name(state) {
            format!("推送到 {name}")
        } else if state.devices.len() > 1 {
            "请在上方选择要推送的手环".into()
        } else {
            "推送时会自动检查手环连接".into()
        }));
        primary_actions(state, busy)
    };
    group()
        .prop("key", "footer")
        .gap(0)
        .flex_shrink(0.0)
        .child(rule())
        .child(
            El::new(Ty::Grid, None)
                .width_full()
                .min_width(0)
                .padding(16)
                .gap(16)
                .grid_template_columns("minmax(0, 1fr) auto")
                .child(message)
                .child(controls),
        )
}
fn build(state: &State) -> El {
    let busy = state.collecting || state.preparing || state.transfer.is_some();
    let active =
        state.collecting || state.preparing || state.transfer.as_ref().is_some_and(|t| !t.paused);
    let mut page = group()
        .prop("key", "page")
        .padding(16)
        .gap(16)
        .child(paragraph("随身新闻", 24))
        .child(devices(state, busy, active))
        .child(settings(state, busy));
    if let Some(message) = &state.toast {
        page = page.child(
            group()
                .prop("key", "fetch-toast")
                .padding(14)
                .radius(CONTROL_RADIUS)
                .bg("var(--red-a3)")
                .border(1, "var(--red-a7)")
                .child(paragraph(message, 14).text_color("var(--red-11)")),
        );
    }
    page = page.child(articles(state, busy));
    if !state.errors.is_empty() {
        page = page.child(errors(state));
    }
    page = page.child(
        line()
            .prop("key", "credits")
            .child(minor("最多100篇 · 单批512 KB").flex_grow(1.0))
            .child(action("开源许可", "license", busy).prop("variant", "ghost")),
    );
    let scroll = El::new(Ty::ScrollArea, None)
        .prop("key", "scroll")
        .prop("scrollbars", "vertical")
        .min_width(0)
        .width_full()
        .height_full()
        .min_height(0)
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .child(page);
    group()
        .prop("key", "root")
        .relative()
        .gap(0)
        .height_full()
        .min_height(0)
        .child(plugin_styles())
        .child(scroll)
        .child(footer(state, busy, active))
}
pub fn render() {
    let (root, tree) = app::with(|state| (state.root.clone(), build(state)));
    if !root.is_empty() {
        ui::render(&root, tree);
    }
}
