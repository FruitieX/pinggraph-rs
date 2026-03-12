pub mod icmp;
pub mod udp;

use chrono::{DateTime, Local};
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Resolve hostname to IP address
pub fn resolve_host(host: &str) -> anyhow::Result<IpAddr> {
    // First try parsing as IP address
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }

    // Try DNS resolution
    let mut addrs = format!("{}:0", host).to_socket_addrs()?;
    if let Some(addr) = addrs.next() {
        return Ok(addr.ip());
    }

    anyhow::bail!("Could not resolve hostname: {}", host)
}

/// Result of a single ping attempt
#[derive(Debug, Clone)]
pub struct PingResult {
    /// Sequence number
    pub seq: u64,
    /// Round-trip time (None if timeout/error)
    pub rtt: Option<Duration>,
    /// When this ping was sent (monotonic)
    #[allow(dead_code)]
    pub sent_at: Instant,
    /// When the response was received (if any)
    #[allow(dead_code)]
    pub received_at: Option<Instant>,
    /// Wall-clock timestamp when ping was sent (for display)
    pub timestamp: DateTime<Local>,
    /// Jitter (difference from previous RTT, None if first ping or timeout)
    pub jitter: Option<Duration>,
}

impl PingResult {
    pub fn success(seq: u64, rtt: Duration, sent_at: Instant, prev_rtt: Option<Duration>) -> Self {
        let jitter = prev_rtt.map(|prev| rtt.abs_diff(prev));
        Self {
            seq,
            rtt: Some(rtt),
            sent_at,
            received_at: Some(Instant::now()),
            timestamp: Local::now(),
            jitter,
        }
    }

    pub fn timeout(seq: u64, sent_at: Instant) -> Self {
        Self {
            seq,
            rtt: None,
            sent_at,
            received_at: None,
            timestamp: Local::now(),
            jitter: None,
        }
    }

    #[allow(dead_code)]
    pub fn rtt_ms(&self) -> Option<u64> {
        self.rtt.map(|d| d.as_millis() as u64)
    }

    /// Get RTT in milliseconds as f64 for sub-millisecond precision
    pub fn rtt_ms_f64(&self) -> Option<f64> {
        self.rtt.map(|d| d.as_secs_f64() * 1000.0)
    }

    /// Get jitter in milliseconds as f64
    pub fn jitter_ms_f64(&self) -> Option<f64> {
        self.jitter.map(|d| d.as_secs_f64() * 1000.0)
    }

    /// Format timestamp as HH:MM:SS.mmm
    pub fn timestamp_str(&self) -> String {
        self.timestamp.format("%H:%M:%S%.3f").to_string()
    }
}

/// Trait for ping implementations
pub trait Pinger: Send {
    /// Run the pinger loop on the current thread, sending results through `tx`.
    /// Returns when `stop` is set to `true`.
    fn run(self: Box<Self>, tx: mpsc::Sender<PingResult>, stop: Arc<AtomicBool>);
}

/// Statistics tracker for ping results
#[derive(Debug, Clone, Default)]
pub struct PingStats {
    pub total_sent: u64,
    pub total_received: u64,
    pub total_lost: u64,
    pub min_rtt: Option<Duration>,
    pub max_rtt: Option<Duration>,
    pub sum_rtt: Duration,
}

impl PingStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, result: &PingResult) {
        self.total_sent += 1;

        if let Some(rtt) = result.rtt {
            self.total_received += 1;
            self.sum_rtt += rtt;

            self.min_rtt = Some(match self.min_rtt {
                Some(min) => min.min(rtt),
                None => rtt,
            });

            self.max_rtt = Some(match self.max_rtt {
                Some(max) => max.max(rtt),
                None => rtt,
            });
        } else {
            self.total_lost += 1;
        }
    }

    pub fn avg_rtt(&self) -> Option<Duration> {
        if self.total_received > 0 {
            Some(self.sum_rtt / self.total_received as u32)
        } else {
            None
        }
    }

    pub fn loss_percent(&self) -> f64 {
        if self.total_sent > 0 {
            (self.total_lost as f64 / self.total_sent as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn format_stats(&self) -> String {
        let min = self
            .min_rtt
            .map(|d| format!("{:.1}", d.as_secs_f64() * 1000.0))
            .unwrap_or("-".to_string());
        let avg = self
            .avg_rtt()
            .map(|d| format!("{:.1}", d.as_secs_f64() * 1000.0))
            .unwrap_or("-".to_string());
        let max = self
            .max_rtt
            .map(|d| format!("{:.1}", d.as_secs_f64() * 1000.0))
            .unwrap_or("-".to_string());

        format!(
            "Sent: {} | Rcvd: {} | Lost: {} ({:.1}%) | RTT min/avg/max: {}/{}/{} ms",
            self.total_sent,
            self.total_received,
            self.total_lost,
            self.loss_percent(),
            min,
            avg,
            max
        )
    }
}
