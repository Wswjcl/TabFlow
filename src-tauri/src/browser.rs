/// Browser tab detection via companion extension.
///
/// This module handles communication with the TabFlow browser extension.
/// The extension sends tab data via WebSocket to a local server.
///
/// Architecture:
/// - Desktop app runs a local WebSocket server on 127.0.0.1:19876
/// - Browser extension connects and sends tab updates
/// - This module receives and processes the data
///
/// For now, this is a stub that relies on window title parsing
/// as a fallback mechanism.
use crate::platform::TrackedItem;

/// Start the WebSocket server for browser extension communication
#[allow(dead_code)]
pub fn start_extension_server() {
    // TODO: Implement WebSocket server using tokio-tungstenite
    // - Listen on 127.0.0.1:19876
    // - Accept connections from browser extensions
    // - Receive tab data: { url, title, browser, windowId, tabId }
    // - Forward to monitor/db modules
}

/// Parse tab info from extension message
#[allow(dead_code)]
pub fn parse_extension_message(msg: &str) -> Option<Vec<TrackedItem>> {
    // TODO: Parse JSON messages from browser extension
    // Message format:
    // {
    //   "type": "tabs_update",
    //   "browser": "chrome",
    //   "tabs": [
    //     { "url": "https://...", "title": "...", "tabId": 1, "windowId": 1 }
    //   ]
    // }
    let _ = msg;
    None
}