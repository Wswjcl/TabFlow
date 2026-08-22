use crate::platform::{ItemType, TrackedItem};
use chrono::Utc;
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

/// CDP debug ports to scan
const CDP_PORTS: [i32; 4] = [9222, 9223, 9224, 9225];

/// Timeout for CDP HTTP requests (seconds) — localhost should respond in <1s
const CDP_TIMEOUT_SECS: u64 = 1;

/// Build a reqwest client with a short timeout for CDP requests
fn cdp_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(CDP_TIMEOUT_SECS))
        .build()
        .unwrap_or_default()
}

/// A tab as returned by Chrome DevTools Protocol /json endpoint
#[derive(Debug, Deserialize)]
struct CdpTab {
    #[allow(dead_code)]
    id: String,
    title: String,
    url: String,
    #[serde(rename = "type")]
    tab_type: String,
    #[allow(dead_code)]
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_url: Option<String>,
}

/// Result of scanning a single CDP port
struct CdpScanResult {
    port: i32,
    tabs: Vec<CdpTab>,
}

/// Scan all CDP ports in parallel and return whichever responds.
async fn scan_cdp_ports() -> Vec<CdpScanResult> {
    let client = cdp_client();
    let mut handles = Vec::new();

    for port in CDP_PORTS {
        let url = format!("http://127.0.0.1:{}/json", port);
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            let resp = client.get(&url).send().await;
            match resp {
                Ok(resp) => {
                    if let Ok(tabs) = resp.json::<Vec<CdpTab>>().await {
                        Some(CdpScanResult { port, tabs })
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(Some(r)) = handle.await {
            results.push(r);
        }
    }
    results
}

/// Check if CDP is available (at least one browser has debugging enabled)
#[tauri::command]
pub async fn check_cdp_status() -> bool {
    let client = cdp_client();
    // Check ports in parallel — return true as soon as one succeeds
    let mut handles = Vec::new();
    for port in CDP_PORTS {
        let url = format!("http://127.0.0.1:{}/json", port);
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            client.get(&url).send().await.is_ok()
        }));
    }
    for handle in handles {
        if let Ok(true) = handle.await {
            return true;
        }
    }
    false
}

/// Scan CDP ports in parallel and fetch all open tabs from Chromium-based browsers.
/// Returns tabs with real URLs.
pub async fn fetch_browser_tabs() -> Vec<TrackedItem> {
    let scan_results = scan_cdp_ports().await;
    let mut all_tabs = Vec::new();

    for result in scan_results {
        let CdpScanResult { port, tabs } = result;
        let browser = detect_browser(&tabs, port);
        for tab in tabs {
            // Skip non-page tabs (extensions, devtools, etc.)
            if tab.tab_type != "page" {
                continue;
            }
            // Skip chrome:// and edge:// internal pages
            if tab.url.starts_with("chrome://")
                || tab.url.starts_with("edge://")
                || tab.url.starts_with("about:")
                || tab.url.starts_with("chrome-extension://")
            {
                continue;
            }
            // Skip empty/new tab pages
            if tab.url.is_empty() || tab.url == "about:blank" {
                continue;
            }

            all_tabs.push(TrackedItem {
                id: format!("tab_{}", tab.id),
                title: tab.title.clone(),
                url: Some(tab.url.clone()),
                path: None,
                process_name: format!("{}.exe", browser),
                window_handle: None, // tabs don't have window handles
                item_type: ItemType::BrowserTab,
                browser_name: Some(browser.to_string()),
                last_active_at: Utc::now().to_rfc3339(),
            });
        }
    }

    all_tabs
}

/// Close a browser tab via CDP.
/// The item_id format for CDP tabs is "tab_{cdp_target_id}".
/// We extract the CDP target ID and try `/json/close/{id}` on all CDP ports in parallel.
pub async fn close_cdp_tab(item_id: &str) -> bool {
    let cdp_id = match item_id.strip_prefix("tab_") {
        Some(id) => id,
        None => return false,
    };

    let client = cdp_client();
    let mut handles = Vec::new();
    for port in CDP_PORTS {
        let url = format!("http://127.0.0.1:{}/json/close/{}", port, cdp_id);
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            match client.get(&url).send().await {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            }
        }));
    }
    for handle in handles {
        if let Ok(true) = handle.await {
            return true;
        }
    }
    false
}

/// Activate (focus) a browser tab via CDP.
/// This brings the tab to the foreground in its browser window.
pub async fn focus_cdp_tab(item_id: &str) -> bool {
    let cdp_id = match item_id.strip_prefix("tab_") {
        Some(id) => id,
        None => return false,
    };

    let client = cdp_client();
    let mut handles = Vec::new();
    for port in CDP_PORTS {
        let url = format!("http://127.0.0.1:{}/json/activate/{}", port, cdp_id);
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            match client.get(&url).send().await {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            }
        }));
    }
    for handle in handles {
        if let Ok(true) = handle.await {
            return true;
        }
    }
    false
}

fn detect_browser(tabs: &[CdpTab], _port: i32) -> &'static str {
    for tab in tabs {
        let u = &tab.url;
        if u.starts_with("edge://") || u.starts_with("microsoft-edge://") {
            return "msedge";
        }
    }
    // Look at user agent? For now, assume Chrome if it's a Chromium browser
    // with no Edge-specific pages
    for tab in tabs {
        if tab.url.starts_with("chrome://") {
            return "chrome";
        }
    }
    "chrome" // default to Chrome
}
