// SPINE Agent Bridge — Popup Script

const wsUrlInput = document.getElementById("ws-url");
const statusDot = document.getElementById("status-dot");
const statSent = document.getElementById("stat-sent");
const statRecv = document.getElementById("stat-recv");
const statErr = document.getElementById("stat-err");
const urPreview = document.getElementById("ur-preview");

let currentUR = null;

// ── Status Polling ──

function refreshStatus() {
  chrome.runtime.sendMessage({ action: "getStatus" }, (resp) => {
    if (!resp) return;
    statusDot.className = `dot ${resp.state}`;
    statSent.textContent = resp.stats.sent;
    statRecv.textContent = resp.stats.received;
    statErr.textContent = resp.stats.errors;
  });
}

// ── Buttons ──

document.getElementById("btn-connect").addEventListener("click", () => {
  const url = wsUrlInput.value.trim();
  if (!url) return;
  chrome.runtime.sendMessage({ action: "connect", url });
  setTimeout(refreshStatus, 500);
});

document.getElementById("btn-disconnect").addEventListener("click", () => {
  chrome.runtime.sendMessage({ action: "disconnect" });
  setTimeout(refreshStatus, 200);
});

document.getElementById("btn-extract").addEventListener("click", () => {
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    if (!tabs[0]) return;
    chrome.tabs.sendMessage(tabs[0].id, { action: "getUR" }, (ur) => {
      if (ur) {
        currentUR = ur;
        urPreview.textContent = JSON.stringify(ur, null, 2).substring(0, 2000);
      } else {
        urPreview.textContent = "Failed to extract (page may not be ready)";
      }
    });
  });
});

document.getElementById("btn-send-ur").addEventListener("click", () => {
  if (!currentUR) {
    urPreview.textContent = "Extract UR first";
    return;
  }
  chrome.runtime.sendMessage({ action: "sendUR", ur: currentUR });
  refreshStatus();
});

// ── Status update listener ──

chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type === "status") {
    statusDot.className = `dot ${msg.state}`;
  }
});

// ── Load saved URL ──

chrome.storage.local.get("wsUrl", (data) => {
  if (data.wsUrl) wsUrlInput.value = data.wsUrl;
});

wsUrlInput.addEventListener("change", () => {
  chrome.storage.local.set({ wsUrl: wsUrlInput.value.trim() });
});

// Initial status
refreshStatus();
setInterval(refreshStatus, 3000);
