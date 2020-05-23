use std::time::Duration;

pub const PORT: u32 = 6881;

pub const REQUEST_EXPIRATION: Duration = Duration::from_secs(30);
pub const MAX_OUTSTANDING_REQUESTS_PER_PEER: i32 = 10;
pub const MAX_REQUESTS_PER_TICK_PER_DOWNLOAD: usize = 100;

pub const MAX_PEER_RECONNECT_ATTEMPTS: u32 = 32;
pub const MIN_PEER_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

pub const BLOCK_SIZE: u64 = 16384;

pub const STATE_PERISTENCE_INTERVAL: Duration = std::time::Duration::from_secs(3);

pub const TICK_INTERVAL: Duration = std::time::Duration::from_millis(100);

pub const MAX_INCOMING_MESSAGES_PER_TICK_PER_DOWNLOAD: u32 = 100;

pub const OUTGOING_CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
