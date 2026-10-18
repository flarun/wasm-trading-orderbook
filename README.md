# wasm-trading-orderbook

A zero-backend, client-side Level 2 (L2) Limit Order Book visualizer built with Rust, WebAssembly, and `egui`.

Streams high-frequency market depth updates (100ms) directly from Binance via WebSockets, parses deltas in WebAssembly, and renders a live, hardware-accelerated order depth ladder at 60 FPS in the browser.

## Architecture

* **Engine:** Pure Rust compiled to `wasm32-unknown-unknown`.
* **Rendering:** Immediate-mode WebGL rendering via `eframe` / `egui`.
* **Transport:** Non-blocking asynchronous WebSockets via `ewebsock` over browser-native socket APIs.
* **State Management:** Bidirectional price-level aggregation with real-time volume depth bars and spread calculation.

## Quick Start

1. **Prerequisites:**
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install --locked trunk