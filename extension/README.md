# SPINE Agent Bridge — Browser Extension

> **Status (as of 2026-11-21): not connected to anything.** This extension
> speaks its own JSON text protocol (`{type: ...}` objects) to
> `ws://localhost:9800/ws`. No SPINE component listens on port 9800 (searched
> `src/` and `research/`), and it does not speak the native wire
> (spine-wire v1: binary frames, a HELLO handshake and CBOR bodies, see
> `docs/spec/00-wire.md`). Loaded as-is it will fail to connect and retry every
> five seconds. It is kept in the repository pending an owner decision to
> either give it a v1 client or remove it (`docs/spec/00-wire.md` G11). The
> description below is of what the code does, not of a working integration.

Chrome/Firefox WebExtension (Manifest V3) for human-agent hybrid browsing.

## What it does

- **Connects** to a local SPINE node via WebSocket (`ws://localhost:9800/ws`)
- **Extracts** Unified Representations (UR) from any web page
- **Sends** page data to SPINE agents for analysis
- **Receives** agent commands: highlight elements, extract content, annotate, navigate
- **Bridges** human browsing with AI agent intelligence

## Installation

### Chrome
1. Open `chrome://extensions`
2. Enable "Developer mode"
3. Click "Load unpacked"
4. Select the `extension/` directory

### Firefox
1. Open `about:debugging#/runtime/this-firefox`
2. Click "Load Temporary Add-on"
3. Select `extension/manifest.json`

## Architecture

```
┌──────────────┐     WebSocket     ┌──────────────┐
│  background  │◄──────────────────►│  SPINE Node  │
│  service     │                    │  (localhost)  │
│  worker      │                    └──────────────┘
└──────┬───────┘
       │ chrome.runtime messages
┌──────┴───────┐     ┌──────────────┐
│   content    │     │    popup     │
│   script     │     │    UI        │
│  (per page)  │     │             │
└──────────────┘     └──────────────┘
```

## Agent Commands

| Command     | Description                          |
| ----------- | ------------------------------------ |
| `highlight` | Outline elements matching a selector |
| `extract`   | Get text/HTML from elements          |
| `navigate`  | Navigate tab to a URL                |
| `annotate`  | Place agent annotations on elements  |
| `query`     | Search page text                     |
