# Wry + EIP-1193 injected provider (offline, local assets only)

This is a minimal example of:

- A **Wry** desktop app that embeds a WebView
- Injects a **`window.ethereum`** provider shim (EIP-1193-style)
- Bridges `ethereum.request(...)` to a Rust backend via Wry IPC
- Uses **Alloy** to do offline signing
- **Blocks outbound network**: only loads `app://...` assets and sets `connect-src 'none'`.

## Run

```bash
cargo run
```

## What is sandboxed?

- The WebView only allows navigation to `app://...` and `about:blank`.
- The content is served via Wry's `with_custom_protocol` from embedded assets.
- CSP includes `connect-src 'none'` to prevent `fetch`/XHR/WebSockets.

## Demo wallet

This project uses a hard-coded demo private key purely for local testing.
Do not use this in real software.
