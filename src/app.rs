use crate::{
    astrobox::psys_host_v4::{device, interconnect, register, thirdpartyapp, timer, ui::Event},
    exports::astrobox::psys_plugin_v4::event::EventType,
    news::{self, Article, Saved},
    selection::Filter,
    sources,
    transfer::{self, Reply, Transfer},
    ui,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct State {
    pub root: String,
    pub saved: Saved,
    pub selected: HashSet<String>,
    pub devices: Vec<(String, String)>,
    pub target: String,
    pub status: String,
    pub errors: Vec<String>,
    pub transfer: Option<Transfer>,
    pub collecting: bool,
    pub collection: Option<sources::Collection>,
    pub collected: Vec<Article>,
    pub checked_at: u64,
    pub collecting_categories: Vec<String>,
    pub preparing: bool,
    pub prepare_generation: u64,
    pub readiness: Option<Readiness>,
    pub collecting_source_total: usize,
    pub filter: Filter,
    // Source and channel lists open by themselves only while nothing is chosen in them.
    pub sources_open: bool,
    pub channels_open: bool,
    pub collect_generation: u64,
    pub collect_timer: Option<u64>,
    pub collect_rendered_at: u64,
    pub refreshing_devices: bool,
    pub count_revision: u64,
    pub toast: Option<String>,
    pub toast_generation: u64,
    pub clearing_trash: Option<String>,
}
pub struct Readiness {
    pub last_error: Option<String>,
    pub attempts: u8,
    pub timer: Option<u64>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            root: String::new(),
            saved: Saved::default(),
            selected: HashSet::new(),
            devices: vec![],
            target: String::new(),
            status: "勾选新闻来源和频道后获取新闻".into(),
            errors: vec![],
            transfer: None,
            collecting: false,
            collection: None,
            collected: vec![],
            checked_at: 0,
            collecting_categories: vec![],
            preparing: false,
            prepare_generation: 0,
            readiness: None,
            collecting_source_total: 0,
            filter: Filter::All,
            sources_open: true,
            channels_open: true,
            collect_generation: 0,
            collect_timer: None,
            collect_rendered_at: 0,
            refreshing_devices: false,
            count_revision: 0,
            toast: None,
            toast_generation: 0,
            clearing_trash: None,
        }
    }
}
pub fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    f(&mut STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap())
}
// A push preparation stays current until it is cancelled or superseded.
fn prepare_active(generation: u64) -> bool {
    with(|s| s.preparing && s.prepare_generation == generation)
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn load() {
    let saved = std::fs::read("cache/news.json")
        .ok()
        .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        .unwrap_or_default();
    with(|s| {
        s.saved = saved;
        if s.saved.article_count == 0 {
            s.saved.article_count = news::DEFAULT_ARTICLES;
        }
        s.saved.article_count = s.saved.article_count.clamp(1, news::MAX_ARTICLES);
        s.saved
            .preferences
            .retain(|id| news::CATEGORIES.iter().any(|(key, _)| id == key));
        s.saved.source_preferences = sources::normalize_selection(&s.saved.source_preferences);
        // Settings stay open until the user has picked both sources and channels. A list that
        // already has choices starts folded; the user opens and folds it from then on.
        s.sources_open = s.saved.source_preferences.is_empty();
        s.channels_open = s.saved.preferences.is_empty();
        if s.sources_open || s.channels_open {
            s.saved.settings_open = true;
        }
        s.selected.clear();
    });
    // Rewrite older caches once so they drop fields that are no longer stored.
    persist();
    if let Ok(bytes) = std::fs::read("cache/pending-transfer.json") {
        if let Ok(mut transfer) = serde_json::from_slice::<Transfer>(&bytes) {
            let valid = transfer.messages.len() >= 3
                && transfer.messages.len() <= 2054
                && transfer.protocol == transfer::PROTOCOL
                && transfer.payload.len() <= transfer::MAX_BYTES;
            if valid {
                if let Ok(bytes) = std::fs::read("cache/pending-cursor.json") {
                    if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                        if v["session"] == transfer.session {
                            transfer.chunk_bytes = v["chunkBytes"]
                                .as_u64()
                                .filter(|n| (1..=12000).contains(n))
                                .unwrap_or(12000)
                                as usize;
                            transfer.manifest_ready = v["manifestReady"].as_bool().unwrap_or(false);
                            transfer.current = v["current"]
                                .as_u64()
                                .unwrap_or(0)
                                .min(transfer.messages.len() as u64 - 1)
                                as usize;
                        }
                    }
                }
                transfer.paused = true;
                with(|s| {
                    s.target = transfer.address.clone();
                    s.transfer = Some(transfer);
                    s.status = "发现未完成推送，刷新连接后可继续传输".into();
                });
            } else if transfer.payload.len() > transfer::MAX_BYTES {
                with(|s| s.status = "原待推送批次超过512 KB，请重新选择新闻推送".into());
            }
        }
    }
}
fn write_json(path: &str, value: &impl Serialize) -> Result<(), String> {
    std::fs::create_dir_all("cache").map_err(|e| e.to_string())?;
    let temp = format!("{path}.tmp");
    std::fs::write(&temp, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}
fn save_cursor() -> Result<(), String> {
    let cursor = with(|s| {
        s.transfer
            .as_ref()
            .map(|t| json!({"session": t.session, "current": t.current, "manifestReady": t.manifest_ready, "chunkBytes": t.chunk_bytes}))
    });
    if let Some(cursor) = cursor {
        write_json("cache/pending-cursor.json", &cursor)?;
    }
    Ok(())
}
fn clear_pending() {
    let _ = std::fs::remove_file("cache/pending-transfer.json");
    let _ = std::fs::remove_file("cache/pending-cursor.json");
}
// Nothing is preselected, so fetching waits until the user has chosen sources and channels.
pub fn fetch_problem(s: &State) -> Option<&'static str> {
    let (sources, channels) = (&s.saved.source_preferences, &s.saved.preferences);
    if sources.is_empty() && channels.is_empty() {
        Some("请先勾选新闻来源和新闻频道")
    } else if sources.is_empty() {
        Some("请至少勾选一个新闻来源")
    } else if channels.is_empty() {
        Some("请至少勾选一个新闻频道")
    } else if sources::jobs(channels, sources).is_empty() {
        Some("所选来源不提供这些频道，请调整来源或频道")
    } else {
        None
    }
}
// An unfinished transfer keeps its band; otherwise keep this session's choice while it stays
// connected, or use the only connected band. The choice is not stored between sessions.
fn choose_target(s: &mut State) {
    s.target = if let Some(t) = &s.transfer {
        t.address.clone()
    } else if s.devices.iter().any(|(_, addr)| *addr == s.target) {
        s.target.clone()
    } else if let [(_, only)] = s.devices.as_slice() {
        only.clone()
    } else {
        String::new()
    };
}
fn persist() {
    let data = with(|s| serde_json::to_vec(&s.saved).unwrap());
    let result = (|| {
        std::fs::create_dir_all("cache")?;
        std::fs::write("cache/news.tmp", data)?;
        std::fs::rename("cache/news.tmp", "cache/news.json")
    })();
    if let Err(error) = result {
        with(|s| s.errors.push(format!("本地缓存保存失败: {error}")));
    }
}
pub async fn mount(id: String) {
    with(|s| s.root = id);
    ui::render();
    if with(|s| s.checked_at == 0 && !s.refreshing_devices) {
        refresh_devices().await;
    }
}
pub async fn refresh_devices() {
    if with(|s| std::mem::replace(&mut s.refreshing_devices, true)) {
        return;
    }
    ui::render();
    let list = device::get_connected_device_list().await;
    with(|s| {
        s.refreshing_devices = false;
        s.devices = list.into_iter().map(|d| (d.name, d.addr)).collect();
        s.checked_at = now();
        choose_target(s);
        if !s.collecting && !s.preparing && s.transfer.is_none() {
            s.status = if s.devices.is_empty() {
                "未连接设备".into()
            } else if s.saved.articles.is_empty() {
                "设备已连接，请获取新闻".into()
            } else {
                "设备已连接，可推送所选新闻".into()
            };
        }
    });
    ui::render();
}
pub async fn action(id: &str, kind: Event, payload: &str) {
    if matches!(id, "article-count" | "article-count-input")
        && matches!(
            kind,
            Event::Input | Event::Change | Event::Blur | Event::PointerUp | Event::KeyUp
        )
    {
        if with(|s| s.collecting || s.preparing || s.transfer.is_some()) {
            return;
        }
        let changed = news::article_count(payload).is_some_and(|count| {
            with(|s| {
                let changed = s.saved.article_count != count;
                s.saved.article_count = count;
                changed
            })
        });
        if kind != Event::Input {
            persist();
        }
        // Keep the range mounted during dragging; number edits resync it only on blur.
        if id == "article-count-input" && kind == Event::Blur {
            with(|s| s.count_revision = s.count_revision.wrapping_add(1));
            ui::render();
        } else if id == "article-count" && changed {
            ui::render();
        }
        return;
    }
    if kind != Event::Click {
        return;
    }
    let busy = with(|s| s.collecting || s.preparing || s.transfer.is_some());
    if with(|s| s.clearing_trash.is_some()) {
        return;
    }
    let active =
        with(|s| s.collecting || s.preparing || s.transfer.as_ref().is_some_and(|t| !t.paused));
    match id {
        "clear-trash" if !busy => {
            use crate::astrobox::psys_host_v4::dialog::{
                self, DialogButton, DialogInfo, DialogStyle, DialogType,
            };
            let target = with(|s| s.target.clone());
            if target.is_empty() {
                return;
            }
            let result = dialog::show_dialog(
                DialogType::Alert,
                DialogStyle::Website,
                DialogInfo {
                    title: "清空手环回收站？".into(),
                    content: "将永久删除所选手环回收站中的全部新闻，清空后无法恢复。".into(),
                    buttons: vec![
                        DialogButton {
                            id: "cancel".into(),
                            primary: true,
                            content: "取消".into(),
                        },
                        DialogButton {
                            id: "clear".into(),
                            primary: false,
                            content: "清空".into(),
                        },
                    ],
                },
            )
            .await;
            if result.clicked_btn_id != "clear"
                || with(|s| {
                    s.target != target
                        || s.collecting
                        || s.preparing
                        || s.transfer.is_some()
                        || s.clearing_trash.is_some()
                })
            {
                return;
            }
            let request = format!("trash_{}", now());
            with(|s| {
                s.clearing_trash = Some(request.clone());
                s.status = "正在连接手环并清空回收站".into();
            });
            ui::render();
            let outcome = async {
                let apps = thirdpartyapp::get_thirdparty_app_list(target.clone())
                    .await
                    .map_err(|e| format!("无法检查手环应用: {e}"))?;
                let app = apps
                    .into_iter()
                    .find(|a| a.package_name == transfer::PACKAGE)
                    .ok_or("手环尚未安装随身新闻")?;
                register::register_interconnect_recv(target.clone(), transfer::PACKAGE.into())
                    .await
                    .map_err(|e| format!("无法接收回执: {e}"))?;
                thirdpartyapp::launch_qa(
                    target.clone(),
                    app,
                    format!("hap://app/{}/pages/home", transfer::PACKAGE),
                )
                .await
                .map_err(|e| format!("无法启动手环应用: {e}"))?;
                interconnect::send_qaic_message(
                    target,
                    transfer::PACKAGE.into(),
                    format!(
                        "H{}",
                        json!({"version":2,"type":"clear-trash","requestId":request})
                    ),
                )
                .await
                .map_err(|e| format!("清空请求发送失败: {e}"))
            }
            .await;
            if let Err(error) = outcome {
                with(|s| {
                    s.clearing_trash = None;
                    s.status = error;
                });
            } else {
                timer::set_timeout(1000, &json!({"clearTrash":request,"attempt":1}).to_string());
            }
            ui::render();
        }
        "refresh-devices" if !active => refresh_devices().await,
        "fetch" if !busy => {
            let ok = with(|s| {
                if let Some(problem) = fetch_problem(s) {
                    // Point the user at the unfinished settings instead of fetching nothing.
                    s.status = problem.into();
                    s.saved.settings_open = true;
                    s.sources_open |= s.saved.source_preferences.is_empty();
                    s.channels_open |= s.saved.preferences.is_empty();
                    return false;
                }
                let jobs = sources::jobs(&s.saved.preferences, &s.saved.source_preferences);
                s.collecting = true;
                s.toast = None;
                s.toast_generation = s.toast_generation.wrapping_add(1);
                s.collect_generation += 1;
                s.errors.clear();
                s.collected.clear();
                s.collecting_categories = s.saved.preferences.clone();
                s.collecting_source_total = jobs.len();
                s.collection = Some(sources::Collection::with_categories(
                    jobs,
                    s.saved.article_count,
                    s.collecting_categories.clone(),
                ));
                s.collect_rendered_at = 0;
                s.status = format!(
                    "已完成 0/{} 项采集 · 已获取 0 篇",
                    s.collecting_source_total
                );
                true
            });
            ui::render();
            if ok {
                schedule_collection();
            } else {
                persist();
            }
        }
        "toggle-preferences" if !busy => {
            with(|s| s.saved.settings_open = !s.saved.settings_open);
            persist();
            ui::render();
        }
        "toggle-sources" if !busy => {
            with(|s| s.sources_open = !s.sources_open);
            ui::render();
        }
        "toggle-channels" if !busy => {
            with(|s| s.channels_open = !s.channels_open);
            ui::render();
        }
        "sources-all" | "sources-clear" if !busy => {
            with(|s| {
                s.saved.source_preferences = if id == "sources-all" {
                    sources::providers().iter().map(|p| p.id.clone()).collect()
                } else {
                    vec![]
                };
            });
            persist();
            ui::render();
        }
        "categories-all" | "categories-clear" if !busy => {
            with(|s| {
                s.saved.preferences = if id == "categories-all" {
                    news::CATEGORIES
                        .iter()
                        .map(|(id, _)| (*id).to_owned())
                        .collect()
                } else {
                    vec![]
                }
            });
            persist();
            ui::render();
        }
        "select-all" if !busy => {
            with(|s| s.filter.toggle_all(&s.saved.articles, &mut s.selected));
            ui::render();
        }
        "invert-selection" if !busy => {
            with(|s| s.filter.invert(&s.saved.articles, &mut s.selected));
            ui::render();
        }
        "push" | "resume" if !active && (id == "resume" || !busy) => {
            let generation = with(|s| {
                s.prepare_generation = s.prepare_generation.wrapping_add(1);
                s.preparing = true;
                s.status = "正在检查目标设备和手环应用".into();
                s.prepare_generation
            });
            ui::render();
            let result = start_transfer(id == "resume", generation).await;
            with(|s| {
                if s.prepare_generation == generation {
                    if let Err(error) = result {
                        s.status = error;
                    }
                    s.preparing = false;
                }
            });
            ui::render();
        }
        "pause" => {
            pause_with("已暂停，进度已保存，可继续传输".into());
            ui::render();
        }
        "cancel" => {
            let pending = with(|s| s.transfer.as_ref().map(|t| t.session.clone()));
            if let Some(session) = pending {
                use crate::astrobox::psys_host_v4::dialog::{
                    self, DialogButton, DialogInfo, DialogStyle, DialogType,
                };
                let result = dialog::show_dialog(
                    DialogType::Alert,
                    DialogStyle::Website,
                    DialogInfo {
                        title: "放弃这次推送？".into(),
                        content: "这会清除手机上的传输断点，已保存到手环的新闻不受影响。".into(),
                        buttons: vec![
                            DialogButton {
                                id: "keep".into(),
                                primary: true,
                                content: "保留任务".into(),
                            },
                            DialogButton {
                                id: "discard".into(),
                                primary: false,
                                content: "放弃本批".into(),
                            },
                        ],
                    },
                )
                .await;
                if result.clicked_btn_id != "discard"
                    || !with(|s| s.transfer.as_ref().is_some_and(|t| t.session == session))
                {
                    return;
                }
            }
            let transfer = with(|s| {
                clear_readiness(s);
                s.collecting = false;
                s.collect_generation += 1;
                if let Some(id) = s.collect_timer.take() {
                    timer::clear_timer(id);
                }
                s.collected.clear();
                s.preparing = false;
                s.prepare_generation = s.prepare_generation.wrapping_add(1);
                s.collection = None;
                s.status = "已取消".into();
                s.transfer.take()
            });
            clear_pending();
            if let Some(t) = transfer {
                if let Some(id) = t.timer {
                    timer::clear_timer(id);
                }
                // Interconnect v2 has no remote cancel command. The next replace reclaims staging.
            }
            ui::render();
        }
        "toggle-articles" if !busy => {
            with(|s| s.saved.articles_open = !s.saved.articles_open);
            persist();
            ui::render();
        }
        "license" if !busy => {
            crate::astrobox::psys_host_v4::dialog::open_url(
                "https://github.com/ourongxing/newsnow/blob/main/LICENSE",
            );
        }
        _ if !busy && id.starts_with("filter:") => {
            if let Some(filter) = Filter::from_id(&id[7..]) {
                with(|s| s.filter = filter);
                ui::render();
            }
        }
        _ if !busy && id.starts_with("count:") => {
            if let Some(count) = news::article_count(&id[6..]) {
                with(|s| {
                    s.saved.article_count = count;
                    // Remount the live range so it shows the preset.
                    s.count_revision = s.count_revision.wrapping_add(1);
                });
                persist();
                ui::render();
            }
        }
        _ if !busy && id.starts_with("source:") => {
            let key = &id[7..];
            if !sources::providers().iter().any(|p| p.id == key) {
                return;
            }
            with(|s| {
                if s.saved.source_preferences.iter().any(|p| p == key) {
                    s.saved.source_preferences.retain(|p| p != key);
                } else {
                    s.saved.source_preferences.push(key.into());
                }
            });
            persist();
            ui::render();
        }
        _ if !busy && id.starts_with("category:") => {
            let key = &id[9..];
            if !news::CATEGORIES.iter().any(|(c, _)| *c == key) {
                return;
            }
            with(|s| {
                if s.saved.preferences.iter().any(|p| p == key) {
                    s.saved.preferences.retain(|p| p != key);
                } else {
                    s.saved.preferences.push(key.into());
                }
            });
            persist();
            ui::render();
        }
        _ if !busy && id.starts_with("device:") => {
            let address = &id[7..];
            with(|s| {
                if s.devices.iter().any(|(_, addr)| addr == address) {
                    s.target = address.into();
                }
            });
            ui::render();
        }
        _ if !busy && id.starts_with("article:") => {
            with(|s| {
                let key = &id[8..];
                if s.saved.articles.iter().any(|a| a.id == key) && !s.selected.remove(key) {
                    s.selected.insert(key.into());
                }
            });
            ui::render();
        }
        _ if !busy && id.starts_with("open:") => {
            let url = with(|s| {
                s.saved
                    .articles
                    .iter()
                    .find(|a| a.id == id[5..])
                    .map(|a| a.url.clone())
            });
            if let Some(url) = url {
                crate::astrobox::psys_host_v4::dialog::open_url(&url);
            }
        }
        _ => {}
    }
}

fn schedule_collection() {
    with(|s| {
        s.collect_timer = Some(timer::set_timeout(
            30,
            &json!({"collect": s.collect_generation}).to_string(),
        ));
    });
}
fn collect_next(generation: u64) {
    let progress = with(|s| {
        if !s.collecting || s.collect_generation != generation {
            return None;
        }
        s.collect_timer = None;
        let collection = s.collection.as_mut()?;
        let results = collection.poll();
        let warnings = collection.take_warnings();
        let more = !collection.is_done();
        let completed = collection.completed;
        let any_available = completed > collection.failed;
        // Refresh elapsed time even while every source is still waiting for its first byte.
        // Limit renders to once per second unless a source has just completed.
        let checked_at = now();
        let changed = !results.is_empty()
            || s.collect_rendered_at == 0
            || checked_at.saturating_sub(s.collect_rendered_at) >= 1000;
        s.errors.extend(warnings);
        for (index, result) in results {
            match result {
                Ok(articles) => s.collected.extend(articles),
                Err(error) => s.errors.push(format!(
                    "{}: {error}",
                    sources::provider_name(&sources::feeds()[index].provider)
                )),
            }
        }
        if changed {
            s.collect_rendered_at = checked_at;
            s.status = format!(
                "已完成 {completed}/{} 项采集 · 已获取 {} 篇",
                s.collecting_source_total,
                s.collected.len()
            );
        }
        Some((more, changed, any_available))
    });
    let Some((more, changed, any_available)) = progress else {
        return;
    };
    if more {
        schedule_collection();
        if changed {
            ui::render();
        }
        return;
    } else {
        let count = with(|s| {
            s.collecting = false;
            s.collection = None;
            if s.collected.is_empty() {
                s.status = if any_available {
                    "来源已读取，但本次没有符合条件的完整新闻，已保留上次结果"
                } else {
                    "获取失败，已保留上次新闻，可重试"
                }
                .into();
                return 0;
            }
            s.saved.articles = news::collect_batch_limit(
                std::mem::take(&mut s.collected),
                &s.collecting_categories,
                s.saved.article_count,
            );
            s.saved.fetched_at = now();
            s.selected = s.saved.articles.iter().map(|a| a.id.clone()).collect();
            s.filter = Filter::All;
            s.saved.settings_open = false;
            s.sources_open = false;
            s.channels_open = false;
            let count = s.saved.articles.len();
            s.status = if count == 0 {
                format!(
                    "来源获取成功，但所选频道暂无匹配新闻{}",
                    if s.errors.is_empty() {
                        ""
                    } else {
                        "；部分条目未补齐，详见获取提示"
                    }
                )
            } else {
                format!(
                    "已获取 {count} 条，已全选{}{}",
                    if count < s.saved.article_count {
                        "；来源可用新闻不足所选篇数"
                    } else {
                        ""
                    },
                    if s.errors.is_empty() {
                        ""
                    } else {
                        "；部分条目未补齐，详见获取提示"
                    }
                )
            };
            count
        });
        with(|s| {
            if count < s.saved.article_count {
                s.toast = Some(format!(
                    "本次获取 {count} 篇，少于设置的 {} 篇",
                    s.saved.article_count
                ));
                s.toast_generation = s.toast_generation.wrapping_add(1);
                timer::set_timeout(4000, &json!({"toast": s.toast_generation}).to_string());
            }
        });
        persist();
    }
    ui::render();
}
async fn start_transfer(resume: bool, generation: u64) -> Result<(), String> {
    let articles = with(|s| {
        s.saved
            .articles
            .iter()
            .filter(|a| s.selected.contains(&a.id))
            .cloned()
            .collect::<Vec<_>>()
    });
    if articles.is_empty() && !resume {
        return Err("请先选择要推送的新闻".into());
    }
    // Refresh connections here so pushing never needs a separate refresh tap.
    let connected = device::get_connected_device_list().await;
    if !prepare_active(generation) {
        return Err("已取消".into());
    }
    let target = with(|s| {
        s.devices = connected
            .iter()
            .map(|d| (d.name.clone(), d.addr.clone()))
            .collect();
        s.checked_at = now();
        choose_target(s);
        s.target.clone()
    });
    if target.is_empty() {
        return Err(if connected.is_empty() {
            "未连接手环，请连接后再推送".into()
        } else {
            "请选择要推送的手环".into()
        });
    }
    let mut transfer = if resume {
        with(|s| s.transfer.clone()).ok_or("没有待继续的推送")?
    } else {
        // The wearable is text-only, so batches carry no pictures.
        Transfer::new(target.clone(), articles, now())?
    };
    let target = transfer.address.clone();
    if !connected.iter().any(|d| d.addr == target) {
        return Err("目标手环已断开，请连接后继续".into());
    }
    if !prepare_active(generation) {
        return Err("已取消".into());
    }
    let apps = thirdpartyapp::get_thirdparty_app_list(target.clone())
        .await
        .map_err(|e| format!("无法检查手环应用: {e}"))?;
    let app = apps
        .into_iter()
        .find(|a| a.package_name == transfer::PACKAGE)
        .ok_or("目标设备尚未安装随身新闻 RPK")?;
    if !prepare_active(generation) {
        return Err("已取消".into());
    }
    register::register_interconnect_recv(target.clone(), transfer::PACKAGE.into())
        .await
        .map_err(|e| format!("接收回执权限不可用: {e}"))?;
    // Resolve the installed fingerprint through the documented host API.
    if !prepare_active(generation) {
        return Err("已取消".into());
    }
    transfer.restart();
    write_json("cache/pending-transfer.json", &transfer)
        .map_err(|e| format!("保存断点任务失败: {e}"))?;
    let session = transfer.session.clone();
    let run = transfer.run;
    with(|s| {
        clear_readiness(s);
        s.transfer = Some(transfer);
        // Install the readiness state before the fire-and-forget native launch.
        s.readiness = Some(Readiness {
            last_error: None,
            attempts: 0,
            timer: None,
        });
        s.status = "正在启动手环应用".into();
    });
    ui::render();
    // Launch before probing: a closed app cannot acknowledge a readiness request.
    let launch_uri = format!(
        "hap://app/{}/pages/receive?incomingId={}",
        transfer::PACKAGE,
        with(|s| s.transfer.as_ref().unwrap().batch_id.clone())
    );
    let launched = thirdpartyapp::launch_qa(target, app, launch_uri).await;
    if !with(|s| {
        s.preparing
            && s.prepare_generation == generation
            && s.transfer
                .as_ref()
                .is_some_and(|t| !t.paused && t.session == session && t.run == run)
    }) {
        return Ok(());
    }
    if let Err(error) = launched {
        pause_with(format!("无法启动手环应用: {error}"));
        return Ok(());
    }
    with(|s| {
        if s.readiness.is_some() {
            s.status = "正在确认手环接收通道".into();
        }
    });
    send_probe().await;
    Ok(())
}
async fn send_probe() {
    let current = with(|s| {
        let t = s.transfer.as_ref().filter(|t| !t.paused)?;
        let ready = s.readiness.as_mut()?;
        ready.attempts += 1;
        Some((
            t.address.clone(),
            t.session.clone(),
            t.run,
            t.handshake(),
            ready.attempts,
        ))
    });
    let Some((address, session, run, message, attempt)) = current else {
        return;
    };
    let timeout = timer::set_timeout(
        750,
        &json!({"probe":session,"run":run,"attempt":attempt}).to_string(),
    );
    with(|s| {
        if let Some(ready) = s.readiness.as_mut() {
            if let Some(old) = ready.timer.replace(timeout) {
                timer::clear_timer(old);
            }
        }
    });
    if let Err(e) =
        interconnect::send_qaic_message(address, transfer::PACKAGE.into(), message).await
    {
        with(|s| {
            if s.transfer
                .as_ref()
                .is_some_and(|t| !t.paused && t.session == session && t.run == run)
            {
                if let Some(ready) = s.readiness.as_mut().filter(|r| r.attempts == attempt) {
                    // The app may still be starting. Keep retrying until readiness times out.
                    ready.last_error = Some(e);
                }
            }
        });
    }
    ui::render();
}
fn clear_readiness(s: &mut State) {
    if let Some(ready) = s.readiness.take() {
        if let Some(id) = ready.timer {
            timer::clear_timer(id);
        }
    }
}
fn arm_transfer_timeout() {
    with(|s| {
        if let Some(t) = s.transfer.as_mut().filter(|t| !t.paused) {
            t.generation += 1;
            let payload = json!({"retry":t.session,"generation":t.generation}).to_string();
            if let Some(old) = t
                .timer
                .replace(timer::set_timeout(t.acknowledgement_timeout_ms(), &payload))
            {
                timer::clear_timer(old);
            }
        }
    });
}
async fn send_current() {
    let start = with(|s| {
        let Some(t) = s.transfer.as_mut().filter(|t| !t.paused && !t.pumping) else {
            return None;
        };
        t.pumping = true;
        Some((t.session.clone(), t.run))
    });
    let Some((pump_session, pump_run)) = start else {
        return;
    };
    arm_transfer_timeout();
    loop {
        let packet = with(|s| {
            let t = s
                .transfer
                .as_mut()
                .filter(|t| !t.paused && t.session == pump_session && t.run == pump_run)?;
            let (index, message) = t.next_packet()?;
            s.status = format!("正在推送 {}% · 请保持手环应用打开", t.percent());
            Some((t.address.clone(), t.session.clone(), index, message))
        });
        let Some((address, session, _index, message)) = packet else {
            break;
        };
        // Host sends stay ordered, while up to four packets await application ACKs.
        arm_transfer_timeout();
        if let Err(error) =
            interconnect::send_qaic_message(address, transfer::PACKAGE.into(), message).await
        {
            if with(|s| {
                s.transfer
                    .as_ref()
                    .is_some_and(|t| !t.paused && t.session == session && t.run == pump_run)
            }) {
                pause_with(format!("连接中断: {error}，进度已保存，刷新后继续"));
            }
            break;
        }
    }
    with(|s| {
        if let Some(t) = s
            .transfer
            .as_mut()
            .filter(|t| t.session == pump_session && t.run == pump_run)
        {
            t.pumping = false;
        }
    });
    ui::render();
}

fn stop_with(message: String) {
    with(|s| {
        clear_readiness(s);
        if let Some(t) = s.transfer.take() {
            if let Some(id) = t.timer {
                timer::clear_timer(id);
            }
        }
        s.status = message;
    });
    clear_pending();
}
fn pause_with(message: String) {
    with(|s| {
        clear_readiness(s);
        if let Some(t) = s.transfer.as_mut() {
            t.paused = true;
            if let Some(id) = t.timer.take() {
                timer::clear_timer(id);
            }
        }
        s.status = message;
    });
    if let Err(error) = save_cursor() {
        with(|s| {
            s.errors
                .push(format!("进度记录失败，恢复时将重新向手环确认断点: {error}"))
        });
    }
}
pub async fn event(kind: EventType, payload: String) {
    match kind {
        EventType::InterconnectMessage => {
            let Some(message) = transfer::decode_message(&payload) else {
                return;
            };
            if message["kind"] == "H" && message["type"] == "clear-trash-result" {
                let changed = with(|s| {
                    if !s
                        .clearing_trash
                        .as_ref()
                        .is_some_and(|id| message["requestId"] == *id)
                    {
                        return false;
                    }
                    s.clearing_trash = None;
                    s.status = if message["ok"] == true {
                        format!(
                            "回收站已清空，删除 {} 批新闻",
                            message["count"].as_u64().unwrap_or(0)
                        )
                    } else {
                        format!(
                            "清空失败：{}",
                            message["message"].as_str().unwrap_or("请重试")
                        )
                    };
                    true
                });
                if changed {
                    ui::render();
                }
                return;
            }
            let is_ready = with(|s| {
                s.readiness.is_some()
                    && s.transfer.as_ref().is_some_and(|t| {
                        !t.paused
                            && message["kind"] == "H"
                            && message["type"] == "response"
                            && message["replyTo"] == t.handshake_id()
                    })
            });
            if is_ready {
                let result = with(|s| s.transfer.as_mut().unwrap().negotiate(&message));
                if let Err(error) = result {
                    pause_with(error);
                    ui::render();
                    return;
                }
                println!("[news-transfer] wearable readiness accepted");
                with(clear_readiness);
                send_current().await;
                return;
            }
            if with(|s| s.readiness.is_some()) {
                // Do not hide archive/storage failures behind a generic readiness timeout.
                let failure = with(|s| {
                    s.transfer.as_mut().and_then(|t| {
                        if message["kind"] == "E" && message["themeId"] == t.batch_id {
                            match t.reply(&message) {
                                Reply::Error(error) => Some(error),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    })
                });
                if let Some(error) = failure {
                    pause_with(error);
                    ui::render();
                }
                return;
            }
            let reply = with(|s| {
                s.transfer
                    .as_mut()
                    .map(|t| t.reply(&message))
                    .unwrap_or(Reply::Ignored)
            });
            match reply {
                Reply::Next => {
                    if let Err(e) = save_cursor() {
                        pause_with(format!("保存断点失败: {e}"));
                        ui::render();
                    } else {
                        send_current().await;
                    }
                }
                Reply::Complete => {
                    println!("[news-transfer] wearable commit confirmed");
                    stop_with("推送成功，手环已接收并保存这批新闻".into());
                    ui::render();
                }
                Reply::Error(e) => {
                    println!("[news-transfer] {e}");
                    pause_with(format!("手环接收暂停: {e}，可继续或放弃此批"));
                    ui::render();
                }
                Reply::Ignored => {}
            }
        }
        EventType::Timer => {
            let value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
            let inner = value["payload"].as_str().unwrap_or(&payload);
            if let Ok(retry) = serde_json::from_str::<Value>(inner) {
                if let Some(request) = retry["clearTrash"].as_str() {
                    if !with(|s| s.clearing_trash.as_deref() == Some(request)) {
                        return;
                    }
                    let attempt = retry["attempt"].as_u64().unwrap_or(15);
                    if attempt < 15 {
                        timer::set_timeout(
                            1000,
                            &json!({"clearTrash":request,"attempt":attempt + 1}).to_string(),
                        );
                        let target = with(|s| s.target.clone());
                        let _ = interconnect::send_qaic_message(
                            target,
                            transfer::PACKAGE.into(),
                            format!(
                                "H{}",
                                json!({"version":2,"type":"clear-trash","requestId":request})
                            ),
                        )
                        .await;
                        return;
                    }
                    let changed = with(|s| {
                        if s.clearing_trash.as_deref() != Some(request) {
                            return false;
                        }
                        s.clearing_trash = None;
                        s.status = "未收到清空结果，请确认手环连接并安装新版应用后重试".into();
                        true
                    });
                    if changed {
                        ui::render();
                    }
                    return;
                }
                if let Some(generation) = retry["toast"].as_u64() {
                    let changed =
                        with(|s| generation == s.toast_generation && s.toast.take().is_some());
                    if changed {
                        ui::render();
                    }
                    return;
                }
                if let Some(generation) = retry["collect"].as_u64() {
                    collect_next(generation);
                    return;
                }
                if retry.get("probe").is_some() {
                    let pending = with(|s| {
                        s.transfer
                            .as_ref()
                            .zip(s.readiness.as_ref())
                            .is_some_and(|(t, r)| {
                                !t.paused
                                    && retry["probe"] == t.session
                                    && retry["run"].as_u64() == Some(t.run)
                                    && retry["attempt"].as_u64() == Some(r.attempts as u64)
                            })
                    });
                    if !pending {
                        return;
                    }
                    let (attempts, last_error) = with(|s| {
                        let r = s.readiness.as_mut().unwrap();
                        // This timeout has fired; it is no longer a cancellable pending timer.
                        r.timer = None;
                        (r.attempts, r.last_error.clone())
                    });
                    if attempts >= 20 {
                        let detail = last_error
                            .map(|e| format!("，最后一次发送失败: {e}"))
                            .unwrap_or_default();
                        pause_with(format!(
                            "未收到手环就绪回执{detail}，进度已保留，请继续传输"
                        ));
                        ui::render();
                    } else {
                        send_probe().await;
                    }
                    return;
                }
                let action = with(|s| {
                    let Some(t) = s.transfer.as_mut() else {
                        return 0;
                    };
                    if t.paused {
                        return 0;
                    }
                    if retry["retry"] != t.session
                        || retry["generation"].as_u64() != Some(t.generation)
                    {
                        return 0;
                    }
                    t.timer = None;
                    if t.retry_pending() { 1 } else { 2 }
                });
                if action == 1 {
                    send_current().await;
                } else if action == 2 {
                    pause_with("连接超时，进度已保存，请刷新连接后继续传输".into());
                    ui::render();
                }
            }
        }
        _ => {}
    }
}
