use crate::platform::{ItemType, TrackedItem};
use chrono::Utc;
use serde::Deserialize;
use std::time::Duration;

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

/// Response of /json/version — the `Browser` field tells the exact browser
/// ("Edg/137.0.0.0", "Chrome/138.0.0.0", "OPR/…"), more reliable than
/// guessing from open tabs.
#[derive(Debug, Deserialize)]
struct CdpVersion {
    #[serde(default)]
    browser: String,
}

/// Result of scanning a single CDP port
struct CdpScanResult {
    tabs: Vec<CdpTab>,
    /// Browser identifier from /json/version ("msedge" / "chrome" / …),
    /// empty when the version endpoint did not respond.
    browser: String,
}

/// Scan all CDP ports in parallel and return whichever responds.
async fn scan_cdp_ports() -> Vec<CdpScanResult> {
    let client = cdp_client();
    let mut handles = Vec::new();

    for port in CDP_PORTS {
        let url = format!("http://127.0.0.1:{}/json", port);
        let version_url = format!("http://127.0.0.1:{}/json/version", port);
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            let resp = client.get(&url).send().await;
            match resp {
                Ok(resp) => {
                    if let Ok(tabs) = resp.json::<Vec<CdpTab>>().await {
                        let browser = match client.get(&version_url).send().await {
                            Ok(r) => r
                                .json::<CdpVersion>()
                                .await
                                .ok()
                                .map(|v| v.browser)
                                .unwrap_or_default(),
                            Err(_) => String::new(),
                        };
                        Some(CdpScanResult { tabs, browser })
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

/// Check whether a specific debug port answers and reports a browser.
pub async fn is_debug_port_open(port: i32) -> bool {
    cdp_client()
        .get(format!("http://127.0.0.1:{}/json/version", port))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Scan CDP ports in parallel and fetch all open tabs from Chromium-based browsers.
/// Returns tabs with real URLs.
pub async fn fetch_browser_tabs() -> Vec<TrackedItem> {
    let scan_results = scan_cdp_ports().await;
    // One timestamp for the whole scan so ordering stays deterministic
    let now = Utc::now().to_rfc3339();
    let mut all_tabs = Vec::new();

    for result in scan_results {
        let CdpScanResult { tabs, browser } = result;
        let browser = identify_browser(&browser, &tabs);
        for tab in tabs {
            // Skip non-page tabs (extensions, devtools, etc.)
            if tab.tab_type != "page" {
                continue;
            }
            // Skip only URL-less tabs: internal pages (new-tab, settings)
            // are real tabs and count, same as on the extension channel.
            if tab.url.is_empty() {
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
                last_active_at: now.clone(),
                task_ids: Vec::new(),
            });
        }
    }

    all_tabs
}

/// Map the /json/version "Browser" string to a process identifier.
/// Falls back to tab-based heuristics when the version string is missing.
fn identify_browser(version_browser: &str, tabs: &[CdpTab]) -> &'static str {
    let v = version_browser.to_lowercase();
    if v.contains("edg") {
        return "msedge";
    }
    if v.contains("opr") {
        return "opera";
    }
    if v.contains("vivaldi") {
        return "vivaldi";
    }
    if v.contains("brave") {
        return "brave";
    }
    if !v.is_empty() {
        return "chrome";
    }

    // Fallback heuristics on tab URLs
    for tab in tabs {
        if tab.url.starts_with("edge://") || tab.url.starts_with("microsoft-edge://") {
            return "msedge";
        }
    }
    "chrome"
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
