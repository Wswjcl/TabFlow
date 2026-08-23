//! Browser tab detection via companion extension.
//!
//! A small WebExtension (MV3 service worker) connects to a WebSocket server
//! on 127.0.0.1:19876 and pushes full tab snapshots on every tab event.
//! Unlike CDP this works on normally-running browsers — no debug flag, no
//! restart — and supports any Chromium browser plus Firefox.
//!
//! Pairing: the desktop app generates a random token (see
//! [`pairing_token`]); the user pastes it once into the extension popup.
//! Connections with a wrong token are dropped, so stray local processes
//! can't inject fake tab data.
//!
//! Protocol (JSON over WebSocket):
//!   ext → app: {"type":"hello","token","browser","userAgent"}
//!            {"type":"tabs","tabs":[{tabId,windowId,url,title,active}]}
//!            {"type":"pong"}
//!   app → ext: {"type":"hello_ack"} | {"type":"error","message"}
//!            {"type":"get_tabs"}
//!            {"type":"close_tab","tabId"} | {"type":"activate_tab","tabId"}
//!            {"type":"ping"}   (~20s, also keeps MV3 service workers alive)

use crate::platform::{ItemType, TrackedItem};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

pub const EXTENSION_PORT: u16 = 19876;

/// One tab as reported by the extension.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtTab {
    pub tab_id: i64,
    #[serde(default)]
    pub window_id: i64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub active: bool,
}

struct Conn {
    tabs: Vec<ExtTab>,
    tx: mpsc::UnboundedSender<String>,
}

struct Hub {
    token: String,
    conns: Mutex<HashMap<String, Conn>>, // keyed by browser id ("chrome", "edge", …)
}

static HUB: OnceLock<Hub> = OnceLock::new();

fn hub() -> &'static Hub {
    HUB.get_or_init(|| Hub {
        token: uuid::Uuid::new_v4().simple().to_string(),
        conns: Mutex::new(HashMap::new()),
    })
}

/// Bind and serve the extension WebSocket. Returns immediately; the server
/// runs on a background task. Logs (does not crash) when the port is taken.
pub fn start_extension_server(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", EXTENSION_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Extension server: cannot bind 127.0.0.1:{}: {}", EXTENSION_PORT, e);
                return;
            }
        };
        println!("Extension server listening on ws://127.0.0.1:{}", EXTENSION_PORT);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        handle_connection(app, stream).await;
                    });
                }
                Err(e) => eprintln!("Extension server accept failed: {}", e),
            }
        }
    });

    // Keepalive ping: refreshes MV3 service workers (Chrome 116+ extends the
    // worker lifetime on WS activity) and detects dead connections.
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        interval.tick().await; // first tick fires immediately
        loop {
            interval.tick().await;
            broadcast(json!({"type": "ping"}).to_string()).await;
        }
    });
}

async fn handle_connection(app: tauri::AppHandle, stream: TcpStream) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("Extension server: WS handshake failed: {}", e);
            return;
        }
    };
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // 1. Wait for the hello message and validate the token.
    let hello = match tokio::time::timeout(Duration::from_secs(10), read.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => return,
    };
    let hello: serde_json::Value = match serde_json::from_str(&hello) {
        Ok(v) => v,
        Err(_) => return,
    };
    if hello["type"] != "hello" {
        let _ = write
            .send(Message::Text(json!({"type": "error", "message": "expected hello"}).to_string()))
            .await;
        return;
    }
    let browser = hello["browser"].as_str().unwrap_or("unknown").to_string();
    let token = hello["token"].as_str().unwrap_or_default();
    if token != hub().token {
        let _ = write
            .send(Message::Text(json!({"type": "error", "message": "invalid token"}).to_string()))
            .await;
        eprintln!("Extension server: rejected connection for '{}' (bad token)", browser);
        return;
    }

    // 2. Register (replaces a stale connection for the same browser).
    hub().conns.lock().await.insert(
        browser.clone(),
        Conn { tabs: Vec::new(), tx: tx.clone() },
    );
    let _ = write
        .send(Message::Text(json!({"type": "hello_ack"}).to_string()))
        .await;
    println!("Extension connected: {}", browser);

    // 3. Writer task: forwards outbound messages to the socket.
    let writer = tauri::async_runtime::spawn(async move {
        while let Some(text) = rx.recv().await {
            if write.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // 4. Reader loop: tab snapshots in, events out.
    let mut prev_snapshot: Vec<(i64, String)> = Vec::new();
    while let Some(Ok(msg)) = read.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match value["type"].as_str() {
            Some("tabs") => {
                if let Ok(mut tabs) = serde_json::from_value::<Vec<ExtTab>>(value["tabs"].clone()) {
                    tabs.sort_by_key(|t| t.tab_id);
                    let snapshot: Vec<(i64, String)> =
                        tabs.iter().map(|t| (t.tab_id, t.url.clone())).collect();
                    let changed = snapshot != prev_snapshot;
                    prev_snapshot = snapshot;
                    if let Some(conn) = hub().conns.lock().await.get_mut(&browser) {
                        conn.tabs = tabs;
                    }
                    if changed {
                        use tauri::Emitter;
                        let _ = app.emit("windows-updated", ());
                    }
                }
            }
            Some("pong") => {} // keepalive reply
            _ => {}
        }
    }

    // 5. Deregister (only if this connection is still the registered one).
    let mut conns = hub().conns.lock().await;
    if conns.get(&browser).map(|c| c.tx.same_channel(&tx)).unwrap_or(false) {
        conns.remove(&browser);
        use tauri::Emitter;
        let _ = app.emit("windows-updated", ());
    }
    drop(conns);
    writer.abort();
    println!("Extension disconnected: {}", browser);
}

async fn broadcast(text: String) {
    let conns = hub().conns.lock().await;
    for conn in conns.values() {
        let _ = conn.tx.send(text.clone());
    }
}

/// Send a command to one browser's extension. Ok(()) only means it was
/// queued on a live connection; the UI refresh confirms the effect.
async fn send_to(browser: &str, payload: serde_json::Value) -> bool {
    let conns = hub().conns.lock().await;
    match conns.get(browser) {
        Some(conn) => conn.tx.send(payload.to_string()).is_ok(),
        None => false,
    }
}

// ─── Queries used by commands / monitor ───────────────────

/// Browsers with a live extension connection (even if they currently
/// report zero visible tabs).
pub async fn connected_browsers() -> Vec<String> {
    hub().conns.lock().await.keys().cloned().collect()
}

/// All tabs reported by connected extensions, as TrackedItems.
pub async fn get_extension_tabs() -> Vec<TrackedItem> {
    let now = Utc::now().to_rfc3339();
    let conns = hub().conns.lock().await;
    let mut items = Vec::new();

    for (browser, conn) in conns.iter() {
        for tab in &conn.tabs {
            if is_internal_url(&tab.url) {
                continue;
            }
            items.push(TrackedItem {
                id: format!("ext_{}_{}", browser, tab.tab_id),
                title: if tab.title.is_empty() {
                    tab.url.clone()
                } else {
                    tab.title.clone()
                },
                url: Some(tab.url.clone()),
                path: None,
                process_name: format!("{}.exe", browser_exe(browser)),
                window_handle: None,
                item_type: ItemType::BrowserTab,
                browser_name: Some(browser.clone()),
                last_active_at: now.clone(),
                task_ids: Vec::new(),
            });
        }
    }

    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

/// Close a tab via its extension. Item id format: "ext_{browser}_{tabId}".
pub async fn close_extension_tab(item_id: &str) -> bool {
    match parse_ext_id(item_id) {
        Some((browser, tab_id)) => {
            send_to(&browser, json!({"type": "close_tab", "tabId": tab_id})).await
        }
        None => false,
    }
}

/// Activate (focus) a tab via its extension.
pub async fn focus_extension_tab(item_id: &str) -> bool {
    match parse_ext_id(item_id) {
        Some((browser, tab_id)) => {
            send_to(&browser, json!({"type": "activate_tab", "tabId": tab_id})).await
        }
        None => false,
    }
}

/// Close a browser tab by item id, routing to the extension channel for
/// "ext_*" ids and to CDP otherwise. Single entry point for close paths.
pub async fn close_any_tab(item_id: &str) -> bool {
    if item_id.starts_with("ext_") {
        close_extension_tab(item_id).await
    } else {
        crate::cdp::close_cdp_tab(item_id).await
    }
}

/// Activate a browser tab by item id (extension channel or CDP).
pub async fn focus_any_tab(item_id: &str) -> bool {
    if item_id.starts_with("ext_") {
        focus_extension_tab(item_id).await
    } else {
        crate::cdp::focus_cdp_tab(item_id).await
    }
}

fn parse_ext_id(id: &str) -> Option<(String, i64)> {
    let rest = id.strip_prefix("ext_")?;
    let (browser, tab) = rest.rsplit_once('_')?;
    let tab_id = tab.parse().ok()?;
    if browser.is_empty() {
        return None;
    }
    Some((browser.to_string(), tab_id))
}

fn browser_exe(browser: &str) -> &str {
    match browser {
        "edge" => "msedge",
        "firefox" => "firefox",
        "opera" => "opera",
        "brave" => "brave",
        "vivaldi" => "vivaldi",
        _ => "chrome",
    }
}

fn is_internal_url(url: &str) -> bool {
    url.is_empty()
        || url == "about:blank"
        || url.starts_with("chrome://")
        || url.starts_with("edge://")
        || url.starts_with("about:")
        || url.starts_with("chrome-extension://")
        || url.starts_with("moz-extension://")
}

/// Status for the pairing UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    pub port: u16,
    pub token: String,
    pub connected: Vec<ConnectedBrowser>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedBrowser {
    pub browser: String,
    pub tab_count: usize,
}

#[tauri::command]
pub async fn get_extension_status() -> ExtensionStatus {
    let conns = hub().conns.lock().await;
    ExtensionStatus {
        port: EXTENSION_PORT,
        token: hub().token.clone(),
        connected: conns
            .iter()
            .map(|(browser, conn)| ConnectedBrowser {
                browser: browser.clone(),
                tab_count: conn.tabs.len(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extension_item_ids() {
        assert_eq!(
            parse_ext_id("ext_chrome_123456"),
            Some(("chrome".to_string(), 123456))
        );
        assert_eq!(
            parse_ext_id("ext_edge_42"),
            Some(("edge".to_string(), 42))
        );
        assert_eq!(parse_ext_id("tab_123"), None);
        assert_eq!(parse_ext_id("ext_chrome_"), None);
        assert_eq!(parse_ext_id("ext__7"), None); // empty browser
        assert_eq!(parse_ext_id("ext_chrome_abc"), None);
    }

    #[test]
    fn filters_internal_urls() {
        assert!(is_internal_url("chrome://settings"));
        assert!(is_internal_url("edge://newtab"));
        assert!(is_internal_url(""));
        assert!(!is_internal_url("https://example.com"));
    }

    /// The extension sends camelCase keys (tabId/windowId) — make sure the
    /// serde rename matches the wire format.
    #[test]
    fn deserializes_extension_tab_snapshot() {
        let value = serde_json::json!([
            { "tabId": 5, "windowId": 1, "url": "https://x.com/home", "title": "Home", "active": true },
            { "tabId": 9, "url": "https://y.com" }, // minimal fields
        ]);
        let tabs: Vec<ExtTab> = serde_json::from_value(value).expect("snapshot must parse");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].tab_id, 5);
        assert_eq!(tabs[0].window_id, 1);
        assert!(tabs[0].active);
        assert_eq!(tabs[1].tab_id, 9);
        assert_eq!(tabs[1].window_id, 0); // #[serde(default)]
        assert!(!tabs[1].active);
    }
}
