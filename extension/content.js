// SPINE Agent Bridge — Content Script
// Runs in page context: extracts UR, handles agent commands

(() => {
  "use strict";

  // ── Unified Representation Extraction ──

  function extractUR() {
    const ur = {
      url: location.href,
      title: document.title,
      timestamp: Date.now(),
      headings: [],
      links: [],
      text_blocks: [],
      meta: {},
    };

    // Headings
    document.querySelectorAll("h1, h2, h3, h4, h5, h6").forEach((el) => {
      const text = el.textContent?.trim();
      if (text) {
        ur.headings.push({ level: parseInt(el.tagName[1], 10), text });
      }
    });

    // Links
    document.querySelectorAll("a[href]").forEach((el) => {
      const href = el.getAttribute("href");
      const text = el.textContent?.trim();
      if (href && text) {
        ur.links.push({ href, text: text.substring(0, 200) });
      }
    });

    // Text blocks (paragraphs, list items)
    document.querySelectorAll("p, li, td, blockquote").forEach((el) => {
      const text = el.textContent?.trim();
      if (text && text.length > 20) {
        ur.text_blocks.push(text.substring(0, 1000));
      }
    });

    // Meta tags
    document.querySelectorAll("meta[name], meta[property]").forEach((el) => {
      const name = el.getAttribute("name") || el.getAttribute("property");
      const content = el.getAttribute("content");
      if (name && content) {
        ur.meta[name] = content.substring(0, 500);
      }
    });

    return ur;
  }

  // ── Agent Command Handlers ──

  function handleHighlight(selector, color) {
    try {
      document.querySelectorAll(selector).forEach((el) => {
        el.style.outline = `3px solid ${color}`;
        el.dataset.spineHighlight = "true";
      });
    } catch {
      // Invalid selector — ignore
    }
  }

  function clearHighlights() {
    document.querySelectorAll("[data-spine-highlight]").forEach((el) => {
      el.style.outline = "";
      delete el.dataset.spineHighlight;
    });
  }

  function handleExtract(selector, requestId) {
    try {
      const elements = document.querySelectorAll(selector);
      const data = Array.from(elements).map((el) => ({
        tag: el.tagName.toLowerCase(),
        text: (el.textContent || "").trim().substring(0, 2000),
        html: el.outerHTML.substring(0, 5000),
      }));
      chrome.runtime.sendMessage({ action: "extractResult", requestId, data });
    } catch {
      chrome.runtime.sendMessage({ action: "extractResult", requestId, data: [] });
    }
  }

  function handleAnnotate(annotations) {
    // Remove previous annotations
    document.querySelectorAll(".spine-annotation").forEach((el) => el.remove());

    if (!Array.isArray(annotations)) return;

    for (const ann of annotations) {
      if (!ann.selector || !ann.text) continue;
      try {
        const target = document.querySelector(ann.selector);
        if (!target) continue;
        const badge = document.createElement("div");
        badge.className = "spine-annotation";
        badge.textContent = ann.text;
        Object.assign(badge.style, {
          position: "absolute",
          background: ann.color || "#1a73e8",
          color: "#fff",
          padding: "4px 8px",
          borderRadius: "4px",
          fontSize: "12px",
          zIndex: "999999",
          maxWidth: "300px",
          pointerEvents: "none",
        });
        const rect = target.getBoundingClientRect();
        badge.style.top = `${window.scrollY + rect.top - 24}px`;
        badge.style.left = `${window.scrollX + rect.left}px`;
        document.body.appendChild(badge);
      } catch {
        // Invalid selector — skip
      }
    }
  }

  function handleQuery(query, requestId) {
    // Simple text search in page
    const body = document.body?.textContent || "";
    const lower = body.toLowerCase();
    const q = (query || "").toLowerCase();
    const idx = lower.indexOf(q);
    let snippet = "";
    if (idx >= 0) {
      const start = Math.max(0, idx - 100);
      const end = Math.min(body.length, idx + q.length + 100);
      snippet = body.substring(start, end);
    }
    chrome.runtime.sendMessage({
      action: "extractResult",
      requestId,
      data: { found: idx >= 0, snippet, url: location.href },
    });
  }

  // ── Message Listener ──

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    switch (msg.action) {
      case "getUR":
        sendResponse(extractUR());
        break;
      case "highlight":
        handleHighlight(msg.selector, msg.color);
        sendResponse({ ok: true });
        break;
      case "clearHighlights":
        clearHighlights();
        sendResponse({ ok: true });
        break;
      case "extract":
        handleExtract(msg.selector, msg.requestId);
        sendResponse({ ok: true });
        break;
      case "annotate":
        handleAnnotate(msg.annotations);
        sendResponse({ ok: true });
        break;
      case "query":
        handleQuery(msg.query, msg.requestId);
        sendResponse({ ok: true });
        break;
      default:
        break;
    }
    return true;
  });

  // ── Auto-send UR on page load ──

  const ur = extractUR();
  chrome.runtime.sendMessage({ action: "pageData", data: ur }).catch(() => {});
})();
