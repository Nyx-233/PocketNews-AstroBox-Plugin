pub mod article;
pub mod classification;
pub mod news;
pub mod selection;
pub mod sources;
mod text_model;
pub mod transfer;

#[cfg(target_arch = "wasm32")]
wit_bindgen::generate!({ path: "wit", world: "psys-world-v4", generate_all });
#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(any(target_arch = "wasm32", test))]
mod http;
#[cfg(target_arch = "wasm32")]
mod ui;

#[cfg(target_arch = "wasm32")]
struct NewsPlugin;
#[cfg(target_arch = "wasm32")]
impl exports::astrobox::psys_plugin_v4::lifecycle::Guest for NewsPlugin {
    async fn on_load() {
        app::load();
    }
}
#[cfg(target_arch = "wasm32")]
impl exports::astrobox::psys_plugin_v4::event::Guest for NewsPlugin {
    async fn on_event(
        kind: exports::astrobox::psys_plugin_v4::event::EventType,
        payload: String,
    ) -> String {
        app::event(kind, payload).await;
        String::new()
    }
    async fn on_ui_event(
        id: String,
        kind: exports::astrobox::psys_plugin_v4::event::UiEvent,
        payload: String,
    ) -> String {
        app::action(&id, kind, &payload).await;
        String::new()
    }
    async fn on_ui_render(id: String) {
        app::mount(id).await;
    }
    async fn on_card_render(_: String) {}
}
#[cfg(target_arch = "wasm32")]
export!(NewsPlugin);
