# wasm-trading-orderbook

A client-side Level 2 Limit Order Book visualizer built with Rust, WebAssembly, and `egui`. It streams live market depth directly from Binance via WebSockets.

## Prerequisites

You need Rust, the WebAssembly compiler target, and the Trunk bundler:

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## How to Run

Launch the local development server:

```bash
trunk serve
```

Open your browser to `http://127.0.0.1:8080`.