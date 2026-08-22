// TabFlow Companion — service worker
//
// Connects to the TabFlow desktop app over WebSocket (ws://127.0.0.1:19876),
// pushes full tab snapshots on every tab event, and executes close/activate
// commands coming back from the app.

const PORT = 19876;
const RECONNECT_DELAY_MS = 3000;

let ws = null;
let reconnectTimer = null;
let snapshotTimer = null;

function browserId() {
  const ua = navigator.userAgent;
  if (ua.includes("Edg/")) return "edge";
  if (ua.includes("OPR/")) return "opera";
  if (ua.includes("Vivaldi")) return "vivaldi";
  if (ua.includes("Brave")) return "brave";
  if (ua.includes("Firefox/")) return "firefox";
  return "chrome";
}

async function getToken() {
  const { tabflowToken } = await chrome.storage.local.get("tabflowToken");
  return tabflowToken || "";
}

function connect() {
  if (ws && (ws.readyState === 0 || ws.readyState === 1)) return; // already connecting/open

  getToken().then((token) => {
    if (!token) {
      // Not paired yet — retry in case the user finishes pairing.
      scheduleReconnect();
      return;
    }
    const socket = new WebSocket(`ws://127.0.0.1:${PORT}`);
    ws = socket;
    socket.onopen = () => {
      socket.send(
        JSON.stringify({
          type: "hello",
          token,
          browser: browserId(),
          userAgent: navigator.userAgent,
        })
      );
      broadcastState();
    };
    socket.onmessage = (ev) => handleMessage(ev.data);
    socket.onclose = () => {
      if (ws === socket) ws = null;
      broadcastState();
      scheduleReconnect();
    };
    socket.onerror = () => {
      /* onclose follows */
    };
  });
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, RECONNECT_DELAY_MS);
}

async function handleMessage(data) {
  let msg;
  try {
    msg = JSON.parse(data);
  } catch {
    return;
  }
  switch (msg.type) {
    case "hello_ack":
      sendTabs();
      break;
    case "get_tabs":
      sendTabs();
      break;
    case "close_tab":
      chrome.tabs.remove(msg.tabId).catch(() => {});
      break;
    case "activate_tab":
      chrome.tabs
        .update(msg.tabId, { active: true })
        .then((tab) => chrome.windows.update(tab.windowId, { focused: true }))
        .catch(() => {});
      break;
    case "ping":
      ws?.send(JSON.stringify({ type: "pong" }));
      break;
    case "error":
      console.warn("[TabFlow] server error:", msg.message);
      break;
  }
}

function sendTabs() {
  if (!ws || ws.readyState !== 1) return;
  chrome.tabs.query({}, (tabs) => {
    if (!ws || ws.readyState !== 1) return;
    ws.send(
      JSON.stringify({
        type: "tabs",
        tabs: tabs.map((t) => ({
          tabId: t.id,
          windowId: t.windowId,
          url: t.url || "",
          title: t.title || "",
          active: !!t.active,
        })),
      })
    );
  });
}

// Debounce bursts of tab events (e.g. restoring a session) into one snapshot.
function scheduleTabs() {
  if (snapshotTimer) return;
  snapshotTimer = setTimeout(() => {
    snapshotTimer = null;
    sendTabs();
  }, 250);
}

for (const name of [
  "onCreated",
  "onUpdated",
  "onRemoved",
  "onActivated",
  "onReplaced",
  "onMoved",
  "onAttached",
  "onDetached",
]) {
  chrome.tabs[name]?.addListener(scheduleTabs);
}
for (const name of ["onCreated", "onRemoved", "onFocusChanged"]) {
  chrome.windows[name]?.addListener(scheduleTabs);
}

// MV3 service workers sleep after ~30s idle; the app's 20s ping keeps the
// socket alive, and this alarm revives the worker if it died anyway.
chrome.alarms.create("tabflow-keepalive", { periodInMinutes: 1 });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === "tabflow-keepalive") connect();
});

// Popup communication: current socket state + manual reconnect.
function broadcastState() {
  chrome.runtime
    .sendMessage({ type: "connection-state", readyState: ws ? ws.readyState : -1 })
    .catch(() => {});
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "get-connection-state") {
    sendResponse({ readyState: ws ? ws.readyState : -1 });
  } else if (msg.type === "reconnect") {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (ws) {
      ws.onclose = null; // suppress auto-reconnect for the old socket
      ws.close();
      ws = null;
    }
    connect();
  }
});

connect();
