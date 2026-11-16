//! Binance WebSocket feed: single combined stream per symbol (L2 snapshots +
//! aggregate trades) with automatic reconnection and exponential backoff.

use crate::symbol::Symbol;
use crate::trades::Trade;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::Deserialize;

const MAX_BACKOFF_SECS: u64 = 15;
pub const STALE_AFTER_SECS: f64 = 3.0;

#[derive(Deserialize, Debug)]
pub struct DepthUpdate {
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
}

#[derive(Deserialize, Debug)]
struct AggTradeEvent {
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "T")]
    timestamp_ms: u64,
    /// `true` = buyer was maker -> seller was taker.
    #[serde(rename = "m")]
    is_buyer_maker: bool,
}

#[derive(Deserialize)]
struct Envelope {
    stream: String,
    data: serde_json::Value,
}

#[derive(Debug)]
pub enum FeedEvent {
    Depth(DepthUpdate),
    Trade(Trade),
}

pub fn parse_message(text: &str) -> Option<FeedEvent> {
    let env: Envelope = serde_json::from_str(text).ok()?;
    if env.stream.contains("@depth") {
        serde_json::from_value(env.data).ok().map(FeedEvent::Depth)
    } else if env.stream.ends_with("@aggTrade") {
        let t: AggTradeEvent = serde_json::from_value(env.data).ok()?;
        Some(FeedEvent::Trade(Trade {
            price: t.price.parse().ok()?,
            qty: t.qty.parse().ok()?,
            time_ms: t.timestamp_ms,
            is_buy: !t.is_buyer_maker,
        }))
    } else {
        None
    }
}

pub fn stream_url(symbol: Symbol) -> String {
    let id = symbol.stream_id();
    format!("wss://stream.binance.com:9443/stream?streams={id}@depth20@100ms/{id}@aggTrade")
}

pub fn backoff_secs(attempt: u32) -> u64 {
    (1u64 << attempt.saturating_sub(1).min(4)).min(MAX_BACKOFF_SECS)
}

enum ConnState {
    Connecting,
    Open,
    Backoff { attempt: u32, retry_at: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Health {
    Connecting,
    Live,
    Stale(f64),
    Reconnecting { attempt: u32, in_secs: f64 },
}

pub struct Feed {
    ctx: egui::Context,
    symbol: Symbol,
    conn: Option<(WsSender, WsReceiver)>,
    state: ConnState,
    attempts: u32,
    last_msg_at: Option<f64>,
    pub reconnects: u32,
}

impl Feed {
    pub fn new(ctx: &egui::Context, symbol: Symbol) -> Self {
        let mut feed = Self {
            ctx: ctx.clone(),
            symbol,
            conn: None,
            state: ConnState::Connecting,
            attempts: 0,
            last_msg_at: None,
            reconnects: 0,
        };
        feed.dial(0.0);
        feed
    }

    pub fn set_symbol(&mut self, symbol: Symbol, now: f64) {
        if self.symbol == symbol {
            return;
        }
        self.symbol = symbol;
        self.attempts = 0;
        self.reconnects = 0;
        self.last_msg_at = None;
        self.dial(now);
    }

    pub fn force_reconnect(&mut self, now: f64) {
        self.attempts = 0;
        self.dial(now);
    }

    fn dial(&mut self, now: f64) {
        self.conn = None;
        self.attempts += 1;
        let url = stream_url(self.symbol);
        let ctx = self.ctx.clone();

        match ewebsock::connect_with_wakeup(url, ewebsock::Options::default(), move || {
            ctx.request_repaint();
        }) {
            Ok(pair) => {
                self.conn = Some(pair);
                self.state = ConnState::Connecting;
            }
            Err(err) => {
                log::warn!("WebSocket dial failed: {err}");
                let delay = backoff_secs(self.attempts) as f64;
                self.state = ConnState::Backoff {
                    attempt: self.attempts,
                    retry_at: now + delay,
                };
            }
        }
    }

    pub fn poll(&mut self, now: f64, out: &mut Vec<FeedEvent>) {
        if let ConnState::Backoff { retry_at, .. } = self.state {
            if now >= retry_at {
                self.reconnects += 1;
                self.dial(now);
            }
            return;
        }

        let mut disconnected = false;
        if let Some((_, rx)) = &mut self.conn {
            while let Some(ev) = rx.try_recv() {
                match ev {
                    WsEvent::Opened => {
                        self.state = ConnState::Open;
                        self.attempts = 0;
                        self.last_msg_at = Some(now);
                    }
                    WsEvent::Message(WsMessage::Text(txt)) => {
                        self.state = ConnState::Open;
                        self.last_msg_at = Some(now);
                        if let Some(parsed) = parse_message(&txt) {
                            out.push(parsed);
                        }
                    }
                    WsEvent::Error(err) => {
                        log::warn!("WebSocket error: {err}");
                        disconnected = true;
                        break;
                    }
                    WsEvent::Closed => {
                        disconnected = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        if disconnected {
            self.conn = None;
            let delay = backoff_secs(self.attempts) as f64;
            self.state = ConnState::Backoff {
                attempt: self.attempts,
                retry_at: now + delay,
            };
        }
    }

    pub fn health(&self, now: f64) -> Health {
        match self.state {
            ConnState::Connecting => Health::Connecting,
            ConnState::Open => {
                if let Some(last) = self.last_msg_at {
                    let silent = (now - last).max(0.0);
                    if silent >= STALE_AFTER_SECS {
                        return Health::Stale(silent);
                    }
                }
                Health::Live
            }
            ConnState::Backoff { attempt, retry_at } => Health::Reconnecting {
                attempt,
                in_secs: (retry_at - now).max(0.0),
            },
        }
    }
}