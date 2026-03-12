use super::{PingResult, Pinger};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// ICMP ping implementation using ping_rs
pub struct IcmpPinger {
    target: IpAddr,
    interval_ms: u64,
    timeout_ms: u64,
}

impl IcmpPinger {
    pub fn new(target: IpAddr, interval_ms: u64, timeout_ms: u64) -> Self {
        Self {
            target,
            interval_ms,
            timeout_ms,
        }
    }
}

impl Pinger for IcmpPinger {
    fn run(self: Box<Self>, tx: mpsc::Sender<PingResult>, stop: Arc<AtomicBool>) {
        let mut seq: u64 = 0;
        let interval = Duration::from_millis(self.interval_ms);
        let timeout = Duration::from_millis(self.timeout_ms);
        let mut prev_rtt: Option<Duration> = None;
        let mut next_tick = Instant::now();

        while !stop.load(Ordering::Relaxed) {
            // Wait until the next tick
            let now = Instant::now();
            if now < next_tick {
                std::thread::sleep(next_tick - now);
            }

            // Skip missed ticks (equivalent to MissedTickBehavior::Skip)
            let now = Instant::now();
            while next_tick <= now {
                next_tick += interval;
            }

            seq += 1;
            let sent_at = Instant::now();
            let ping_start = Instant::now();
            let result = ping_rs::send_ping(&self.target, timeout, &[1, 2, 3, 4], None);

            let ping_result = match result {
                Ok(_reply) => {
                    let rtt = ping_start.elapsed();
                    let res = PingResult::success(seq, rtt, sent_at, prev_rtt);
                    prev_rtt = Some(rtt);
                    res
                }
                Err(_) => {
                    prev_rtt = None;
                    PingResult::timeout(seq, sent_at)
                }
            };

            let _ = tx.send(ping_result);
        }
    }
}
