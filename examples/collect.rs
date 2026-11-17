// Run the production collector under a real WASI host, without AstroBox UI/device imports.
#![allow(dead_code)]

#[cfg(target_arch = "wasm32")]
#[path = "../src/article.rs"]
mod article;
#[cfg(target_arch = "wasm32")]
#[path = "../src/classification.rs"]
mod classification;
#[cfg(target_arch = "wasm32")]
#[path = "../src/http.rs"]
mod http;
#[cfg(target_arch = "wasm32")]
#[path = "../src/news.rs"]
mod news;
#[cfg(target_arch = "wasm32")]
#[path = "../src/sources.rs"]
mod sources;
#[cfg(target_arch = "wasm32")]
#[path = "../src/text_model.rs"]
mod text_model;

#[cfg(target_arch = "wasm32")]
mod app {
    pub fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    let list = std::env::args().any(|arg| arg == "--list");
    let allow_partial = std::env::args().any(|arg| arg == "--allow-partial");
    let limit = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--limit=")
                .and_then(|v| v.parse::<usize>().ok())
        })
        .unwrap_or(news::MAX_ARTICLES);
    let ids: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with("--"))
        .collect();
    assert!(!ids.is_empty(), "Pass feed IDs from source-catalog.json");
    let jobs: Vec<_> = ids
        .iter()
        .map(|id| {
            sources::feeds()
                .iter()
                .position(|feed| &feed.id == id)
                .expect("Unknown feed ID")
        })
        .collect();
    let mut collection = sources::Collection::with_limit(jobs, limit);
    let mut articles = vec![];
    let mut last_progress = None;
    while !collection.is_done() {
        for (index, result) in collection.poll() {
            match result {
                Ok(items) => {
                    println!("OK {} articles={}", sources::feeds()[index].id, items.len());
                    if list {
                        for article in &items {
                            println!(
                                "ARTICLE {} date={} chars={} {}",
                                article.category,
                                article.published_at.unwrap_or(0),
                                article.body.chars().count(),
                                article.title
                            );
                        }
                    }
                    articles.extend(items);
                }
                Err(error) => println!("FAIL {}: {error}", sources::feeds()[index].id),
            }
        }
        for warning in collection.take_warnings() {
            println!("WARN {warning}");
        }
        let progress = (collection.completed, collection.elapsed_seconds());
        if last_progress != Some(progress) {
            println!(
                "PROGRESS {}/{} elapsed={}s active={}",
                progress.0,
                ids.len(),
                progress.1,
                collection.active_summary()
            );
            last_progress = Some(progress);
        }
        if !collection.is_done() {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }
    let unique = news::deduplicate(articles);
    println!(
        "DONE sources={} failed={} unique_articles={}",
        collection.completed,
        collection.failed,
        unique.len()
    );
    assert!(!unique.is_empty(), "All sources failed");
    if !allow_partial {
        assert_eq!(collection.failed, 0, "Some sources failed");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("Build with --target wasm32-wasip2 and run with wasmtime -S http -S inherit-network");
}
