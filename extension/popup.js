const $token = document.getElementById("token");
const $status = document.getElementById("status");
const $save = document.getElementById("save");
const $reconnect = document.getElementById("reconnect");

function render(wsState, hasToken) {
  if (!hasToken) {
    $status.className = "status bad";
    $status.textContent = "未配置 Token — 请粘贴后保存";
  } else if (wsState === 1) {
    $status.className = "status ok";
    $status.textContent = "已连接 TabFlow ✓";
  } else if (wsState === 0) {
    $status.className = "status bad";
    $status.textContent = "连接中…（请确认 TabFlow 已运行）";
  } else {
    $status.className = "status bad";
    $status.textContent = "未连接 — 每 3 秒自动重试";
  }
}

function refresh() {
  chrome.runtime.sendMessage({ type: "get-connection-state" }, (res) => {
    const state = res?.readyState ?? -1;
    render(state, !!$token.value.trim());
  });
}

chrome.storage.local.get("tabflowToken").then(({ tabflowToken }) => {
  $token.value = tabflowToken || "";
  refresh();
});

// Track socket state changes reported by the service worker.
chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type === "connection-state" && !document.hidden) {
    render(msg.readyState, !!$token.value.trim());
  }
});

$save.addEventListener("click", async () => {
  const token = $token.value.trim();
  if (!token) return;
  await chrome.storage.local.set({ tabflowToken: token });
  chrome.runtime.sendMessage({ type: "reconnect" });
  setTimeout(refresh, 500);
});

$reconnect.addEventListener("click", () => {
  chrome.runtime.sendMessage({ type: "reconnect" });
  setTimeout(refresh, 500);
});

setInterval(refresh, 2000);
