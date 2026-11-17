use serde_json::Value;
use std::collections::HashSet;

const HOST: &str = include_str!("../wit/deps/astrobox-psys-host-v4.wit");
const PLUGIN: &str = include_str!("../wit/deps/astrobox-psys-plugin-v4.wit");
const WORLD: &str = include_str!("../wit/main.wit");
const UI: &str = include_str!("../src/ui.rs");
const APP: &str = include_str!("../src/app.rs");
const ENTRY: &str = include_str!("../src/lib.rs");

fn enum_values(source: &str, name: &str) -> HashSet<String> {
    source
        .split_once(&format!("enum {name}"))
        .unwrap()
        .1
        .split_once('{')
        .unwrap()
        .1
        .split_once('}')
        .unwrap()
        .0
        .lines()
        .map(|line| {
            line.split("//")
                .next()
                .unwrap()
                .trim()
                .trim_end_matches(',')
        })
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('-', "").to_ascii_lowercase())
        .collect()
}
#[test]
fn manifest_and_exports_use_the_v4_contract() {
    let manifest: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();
    assert_eq!(manifest["api_level"], 4);
    assert_eq!(manifest["wasi_version"], 3);
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    assert!(WORLD.contains("import astrobox:psys-host-v4/ui;"));
    assert!(WORLD.contains("import astrobox:psys-host-v4/canopusconnect;"));
    assert!(ENTRY.contains("world: \"psys-world-v4\""));
    for entry in [
        "on_load",
        "on_event",
        "on_ui_event",
        "on_ui_render",
        "on_card_render",
    ] {
        assert!(ENTRY.contains(&format!("async fn {entry}")), "{entry}");
        let name = entry.replace('_', "-");
        assert!(PLUGIN.contains(&format!("{name}: async func")), "{name}");
    }
    for source in [HOST, PLUGIN] {
        let declarations: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(!declarations.contains("future<"));
    }
    for source in [WORLD, ENTRY, APP, UI] {
        assert!(!source.contains("astrobox:psys-host/"));
        assert!(!source.contains("astrobox:psys-plugin/"));
        assert!(!source.contains("block_on("));
    }
}
#[test]
fn ui_elements_and_events_exist_in_the_official_v4_interface() {
    let elements = enum_values(HOST, "element-type");
    let events = enum_values(HOST, "event");
    for (prefix, allowed) in [("Ty::", elements), ("Event::", events)] {
        for part in UI.split(prefix).skip(1) {
            let name: String = part
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            assert!(
                allowed.contains(&name.to_ascii_lowercase()),
                "{prefix}{name}"
            );
        }
    }
    for action in [
        "fetch",
        "toggle-preferences",
        "categories-all",
        "categories-clear",
        "toggle-sources",
        "toggle-channels",
        "toggle-articles",
        "sources-all",
        "sources-clear",
        "refresh-devices",
        "select-all",
        "invert-selection",
        "push",
        "pause",
        "resume",
        "cancel",
        "license",
    ] {
        assert!(UI.contains(&format!("\"{action}\"")), "UI action {action}");
        assert!(
            APP.contains(&format!("\"{action}\"")),
            "Unhandled action {action}"
        );
    }
    for prefix in [
        "category:",
        "source:",
        "device:",
        "filter:",
        "article:",
        "open:",
        "count:",
    ] {
        assert!(UI.contains(prefix));
        assert!(APP.contains(&format!("starts_with(\"{prefix}\")")));
    }
}
#[test]
fn ui_preserves_host_styles_and_exposes_collection_controls() {
    // The selection surface and live range retain native interaction behavior.
    assert_eq!(UI.matches(".without_default_styles()").count(), 2);
    assert!(!UI.contains("struct Theme"));
    assert!(!UI.contains(".bg(\"#"));
    assert!(!UI.contains(".text_color(\"#"));
    assert!(UI.contains("Ty::Card"));
    assert!(UI.contains(".prop(\"type\", \"range\")"));
    assert!(UI.contains("Ty::Input"));
    assert!(UI.contains("article-count"));
    let input = UI
        .split("let input =")
        .nth(1)
        .unwrap()
        .split("let slider =")
        .next()
        .unwrap();
    let slider = UI
        .split("let slider =")
        .nth(1)
        .unwrap()
        .split("section = section")
        .next()
        .unwrap();
    for control in [input, slider] {
        let control: String = control.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(control.contains(".prop(\"default-value\""));
        assert!(!control.contains(".prop(\"value\""));
        assert!(control.contains(".prop(\"key\""));
    }
    assert!(!UI.contains(".max_width(720)"));
    assert!(UI.contains("minmax(0, 1fr) auto"));
    assert!(UI.contains("minmax(min(100%, 340px), 1fr)"));
    assert!(!UI.contains("\"✓ 已选\""));
    assert!(!UI.contains("\"+ 选择\""));
    assert!(UI.contains("var(--accent-a3)"));
    assert!(UI.contains("var(--accent-9)"));
    assert!(UI.contains("Ty::Separator"));
    assert!(UI.contains("Ty::ScrollArea"));
    assert!(UI.contains("Ty::Progress"));
    assert!(UI.contains("El::new(Ty::Button"));
    assert!(!APP.contains("pub dark:"));
    for unwanted in [
        "POCKET NEWS",
        "把世界",
        "你关心的世界",
        "留给离线的片刻",
        "挑选好内容，一次带走",
        "currentColor",
        "transparent",
    ] {
        assert!(!UI.contains(unwanted), "Unexpected UI detail: {unwanted}");
    }
    // Pushes are always text-only; there is no picture switch in either the UI or the state.
    for removed in ["no-images", "无图模式", "no_images", "Ty::Checkbox"] {
        assert!(!UI.contains(removed) && !APP.contains(removed), "{removed}");
    }
    assert!(!UI.contains("纯文字推送"));
    // The range fill is drawn by the browser, never from a re-rendered gradient.
    assert!(!UI.contains("linear-gradient") && !UI.contains("appearance:none"));
    // Devices are listed one per row and long names wrap.
    let devices = UI
        .split("fn devices")
        .nth(1)
        .unwrap()
        .split("fn article")
        .next()
        .unwrap();
    assert!(devices.contains("pocket-news-wrap") && !devices.contains("choice_grid"));
    // Every fold control is an arrow, not a text label.
    // The words remain only as accessible labels inside fold().
    assert_eq!(UI.matches("\"收起\"").count(), 2);
    assert!(!UI.contains("\"完成\"") && !UI.contains("\"修改\""));
    // The arrows are CSS chevrons, not triangle glyphs.
    assert!(UI.contains("pocket-news-fold-open") && UI.contains("pocket-news-fold-closed"));
    assert!(!UI.contains("\"▲\"") && !UI.contains("\"▼\""));
    assert!(!APP.contains("saved.device"));
    // Fetch and push are both pinned in the footer.
    let footer = UI.split("fn primary_actions").nth(1).unwrap();
    assert!(footer.contains("\"fetch\"") && footer.contains("\"push\""));
}
