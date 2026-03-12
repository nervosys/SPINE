// SPINE Agent Bridge — Background Service Worker
// Manages WebSocket connection to local SPINE node and message routing

const SPINE_DEFAULT_URL = "ws://localhost:9800/ws";
let ws = null;
let connectionState = "disconnected";
let messageQueue = [];
let reconnectTimer = null;
let stats = { sent: 0, received: 0, errors: 0, connected_at: null };

// ── WebSocket Connection ──

function connect(url) {
  if (ws && ws.readyState === WebSocket.OPEN) return;
  const target = url || SPINE_DEFAULT_URL;

  try {
    ws = new WebSocket(target);
  } catch (e) {
    connectionState = "error";
    stats.errors++;
    scheduleReconnect(target);
    return;
  }

  ws.onopen = () => {
    connectionState = "connected";
    stats.connected_at = Date.now();
    clearReconnectTimer();
    // Flush queued messages
    while (messageQueue.length > 0) {
      const msg = messageQueue.shift();
      ws.send(JSON.stringify(msg));
      stats.sent++;
    }
    notifyPopup({ type: "status", state: "connected" });
  };

  ws.onmessage = (event) => {
    stats.received++;
    try {
      const data = JSON.parse(event.data);
      handleAgentMessage(data);
    } catch {
      // Binary or non-JSON — ignore
    }
  };

  ws.onclose = () => {
    connectionState = "disconnected";
    notifyPopup({ type: "status", state: "disconnected" });
    scheduleReconnect(target);
  };

  ws.onerror = () => {
    stats.errors++;
    connectionState = "error";
  };
}

function disconnect() {
  clearReconnectTimer();
  if (ws) {
    ws.close();
    ws = null;
  }
  connectionState = "disconnected";
  notifyPopup({ type: "status", state: "disconnected" });
}

function scheduleReconnect(url) {
  clearReconnectTimer();
  reconnectTimer = setTimeout(() => connect(url), 5000);
}

function clearReconnectTimer() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

// ── Message Handling ──

function sendToAgent(msg) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(msg));
    stats.sent++;
  } else {
    messageQueue.push(msg);
    if (messageQueue.length > 100) messageQueue.shift(); // cap queue
  }
}

function handleAgentMessage(data) {
  // Route agent messages to content scripts
  switch (data.type) {
    case "highlight":
      broadcastToTabs({ action: "highlight", selector: data.selector, color: data.color || "#ff0" });
      break;
    case "extract":
      broadcastToTabs({ action: "extract", selector: data.selector, requestId: data.id });
      break;
    case "navigate":
      if (data.url && isAllowedUrl(data.url)) {
        chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
          if (tabs[0]) chrome.tabs.update(tabs[0].id, { url: data.url });
        });
      }
      break;
    case "annotate":
      broadcastToTabs({ action: "annotate", annotations: data.annotations });
      break;
    case "query":
      broadcastToTabs({ action: "query", query: data.query, requestId: data.id });
      break;
    default:
      break;
  }
}

function isAllowedUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

function broadcastToTabs(msg) {
  chrome.tabs.query({}, (tabs) => {
    for (const tab of tabs) {
      if (tab.id) {
        chrome.tabs.sendMessage(tab.id, msg).catch(() => {});
      }
    }
  });
}

function notifyPopup(msg) {
  chrome.runtime.sendMessage(msg).catch(() => {});
}

// ── Extension Message Listener ──

chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
  switch (request.action) {
    case "connect":
      connect(request.url);
      sendResponse({ ok: true });
      break;
    case "disconnect":
      disconnect();
      sendResponse({ ok: true });
      break;
    case "getStatus":
      sendResponse({
        state: connectionState,
        stats: { ...stats },
        queueLen: messageQueue.length,
      });
      break;
    case "sendUR":
      sendToAgent({ type: "ur", content: request.ur });
      sendResponse({ ok: true });
      break;
    case "pageData":
      // Content script sends extracted page data to agent
      sendToAgent({ type: "page_data", data: request.data });
      sendResponse({ ok: true });
      break;
    case "extractResult":
      // Content script responds to an agent extract request
      sendToAgent({ type: "extract_result", id: request.requestId, data: request.data });
      sendResponse({ ok: true });
      break;
    default:
      sendResponse({ error: "unknown action" });
  }
  return true; // keep message channel open for async
});
