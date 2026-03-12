use super::{PingResult, Pinger};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Magic bytes for UDP ping packets
const MAGIC: &[u8; 4] = b"PING";

/// UDP packet structure (20 bytes total):
/// - Magic: 4 bytes "PING"
/// - Sequence: 8 bytes (u64 big-endian)
/// - Timestamp: 8 bytes (microseconds since start, u64 big-endian)
fn encode_packet(seq: u64, timestamp_us: u64) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0..4].copy_from_slice(MAGIC);
    buf[4..12].copy_from_slice(&seq.to_be_bytes());
    buf[12..20].copy_from_slice(&timestamp_us.to_be_bytes());
    buf
}

fn decode_packet(buf: &[u8]) -> Option<(u64, u64)> {
    if buf.len() < 20 {
        return None;
    }
    if &buf[0..4] != MAGIC {
        return None;
    }
    let seq = u64::from_be_bytes(buf[4..12].try_into().ok()?);
    let timestamp = u64::from_be_bytes(buf[12..20].try_into().ok()?);
    Some((seq, timestamp))
}

/// UDP client pinger
pub struct UdpClientPinger {
    target: SocketAddr,
    interval_ms: u64,
    timeout_ms: u64,
}

impl UdpClientPinger {
    pub fn new(target: SocketAddr, interval_ms: u64, timeout_ms: u64) -> Self {
        Self {
            target,
            interval_ms,
            timeout_ms,
        }
    }
}

impl Pinger for UdpClientPinger {
    fn run(self: Box<Self>, tx: mpsc::Sender<PingResult>, stop: Arc<AtomicBool>) {
        // Bind to matching address family (IPv4 or IPv6)
        let bind_addr = if self.target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = match UdpSocket::bind(bind_addr) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("Failed to bind UDP socket: {}", e);
                return;
            }
        };

        if let Err(e) = socket.connect(self.target) {
            eprintln!("Failed to connect to {}: {}", self.target, e);
            return;
        }

        // Track pending pings for timeout detection
        let pending: Arc<Mutex<HashMap<u64, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
        let start_time = Instant::now();
        let prev_rtt: Arc<Mutex<Option<Duration>>> = Arc::new(Mutex::new(None));
        let timeout_duration = Duration::from_millis(self.timeout_ms);

        // Spawn receiver thread
        let socket_recv = socket.clone();
        let pending_recv = pending.clone();
        let tx_recv = tx.clone();
        let prev_rtt_recv = prev_rtt.clone();
        let stop_recv = stop.clone();

        // Set a read timeout so the receiver thread can check the stop flag
        let _ = socket_recv.set_read_timeout(Some(Duration::from_millis(100)));

        std::thread::spawn(move || {
            let mut buf = [0u8; 32];
            while !stop_recv.load(Ordering::Relaxed) {
                match socket_recv.recv(&mut buf) {
                    Ok(len) => {
                        if let Some((seq, _timestamp)) = decode_packet(&buf[..len]) {
                            let mut pending = pending_recv.lock().unwrap();
                            if let Some(sent_at) = pending.remove(&seq) {
                                let rtt = sent_at.elapsed();
                                let prev = {
                                    let mut guard = prev_rtt_recv.lock().unwrap();
                                    let prev = *guard;
                                    *guard = Some(rtt);
                                    prev
                                };
                                let _ = tx_recv.send(PingResult::success(seq, rtt, sent_at, prev));
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        // Read timeout — loop back to check stop flag
                    }
                    Err(e) => {
                        // Ignore WSAECONNRESET (10054) on Windows - this happens when
                        // the server isn't listening and we get ICMP port unreachable.
                        let is_connection_reset = e.raw_os_error() == Some(10054);
                        if !is_connection_reset {
                            eprintln!("Recv error: {}", e);
                        }
                    }
                }
            }
        });

        // Spawn timeout checker thread
        let pending_timeout = pending.clone();
        let tx_timeout = tx.clone();
        let prev_rtt_timeout = prev_rtt.clone();
        let stop_timeout = stop.clone();

        std::thread::spawn(move || {
            while !stop_timeout.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                let now = Instant::now();
                let mut pending = pending_timeout.lock().unwrap();
                let timed_out: Vec<(u64, Instant)> = pending
                    .iter()
                    .filter(|(_, sent_at)| now.duration_since(**sent_at) > timeout_duration)
                    .map(|(seq, sent_at)| (*seq, *sent_at))
                    .collect();

                for (seq, sent_at) in timed_out {
                    pending.remove(&seq);
                    *prev_rtt_timeout.lock().unwrap() = None;
                    let _ = tx_timeout.send(PingResult::timeout(seq, sent_at));
                }
            }
        });

        // Main send loop — timer based with skip-on-miss
        let mut seq: u64 = 0;
        let interval = Duration::from_millis(self.interval_ms);
        let mut next_tick = Instant::now();

        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now < next_tick {
                std::thread::sleep(next_tick - now);
            }

            // Skip missed ticks
            let now = Instant::now();
            while next_tick <= now {
                next_tick += interval;
            }

            seq += 1;
            let sent_at = Instant::now();
            let timestamp_us = start_time.elapsed().as_micros() as u64;
            let packet = encode_packet(seq, timestamp_us);

            {
                let mut pending = pending.lock().unwrap();
                pending.insert(seq, sent_at);
            }

            if let Err(e) = socket.send(&packet) {
                eprintln!("Send error: {}", e);
            }
        }
    }
}

/// UDP server that echoes ping packets back
pub struct UdpServer {
    bind: Option<String>,
    port: u16,
}

impl UdpServer {
    pub fn new(bind: Option<String>, port: u16) -> Self {
        Self { bind, port }
    }

    fn handle_packet(socket: &UdpSocket, buf: &[u8], len: usize, src: SocketAddr) {
        if len >= 20
            && &buf[0..4] == MAGIC
            && let Err(e) = socket.send_to(&buf[..len], src)
        {
            eprintln!("Failed to send response to {}: {}", src, e);
        }
    }

    pub fn run(&self) -> anyhow::Result<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        ctrlc::set_handler(move || {
            println!("\nShutting down...");
            stop_flag.store(true, Ordering::Relaxed);
        })?;

        // If a specific bind address is provided, use only that
        if let Some(bind_addr) = &self.bind {
            let addr = format!("{}:{}", bind_addr, self.port);
            let socket = UdpSocket::bind(&addr)?;
            socket.set_read_timeout(Some(Duration::from_millis(100)))?;
            println!("UDP ping server listening on {}", addr);
            println!("Press Ctrl+C to stop");

            let mut buf = [0u8; 32];
            while !stop.load(Ordering::Relaxed) {
                match socket.recv_from(&mut buf) {
                    Ok((len, src)) => Self::handle_packet(&socket, &buf, len, src),
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => eprintln!("Recv error: {}", e),
                }
            }
            return Ok(());
        }

        // Default: bind to both IPv4 and IPv6 on all interfaces
        let socket_v4 = UdpSocket::bind(format!("0.0.0.0:{}", self.port))?;
        socket_v4.set_read_timeout(Some(Duration::from_millis(100)))?;

        let socket_v6 = UdpSocket::bind(format!("[::]:{}", self.port)).ok();
        if let Some(ref s) = socket_v6 {
            let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
        }

        if socket_v6.is_some() {
            println!(
                "UDP ping server listening on port {} (IPv4 + IPv6)",
                self.port
            );
        } else {
            println!(
                "UDP ping server listening on port {} (IPv4 only)",
                self.port
            );
        }
        println!("Press Ctrl+C to stop");

        // Spawn IPv4 handler thread
        let stop_v4 = stop.clone();
        let v4_handle = std::thread::spawn(move || {
            let mut buf = [0u8; 32];
            while !stop_v4.load(Ordering::Relaxed) {
                match socket_v4.recv_from(&mut buf) {
                    Ok((len, src)) => Self::handle_packet(&socket_v4, &buf, len, src),
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => eprintln!("Recv error (v4): {}", e),
                }
            }
        });

        // Optionally spawn IPv6 handler thread
        let v6_handle = socket_v6.map(|socket_v6| {
            let stop_v6 = stop.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 32];
                while !stop_v6.load(Ordering::Relaxed) {
                    match socket_v6.recv_from(&mut buf) {
                        Ok((len, src)) => Self::handle_packet(&socket_v6, &buf, len, src),
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => eprintln!("Recv error (v6): {}", e),
                    }
                }
            })
        });

        let _ = v4_handle.join();
        if let Some(h) = v6_handle {
            let _ = h.join();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_encoding() {
        let seq = 12345u64;
        let timestamp = 9876543210u64;
        let packet = encode_packet(seq, timestamp);

        let (decoded_seq, decoded_ts) = decode_packet(&packet).unwrap();
        assert_eq!(seq, decoded_seq);
        assert_eq!(timestamp, decoded_ts);
    }

    #[test]
    fn test_invalid_packet() {
        assert!(decode_packet(&[0; 10]).is_none()); // Too short
        assert!(decode_packet(b"NOPE12345678901234567890").is_none()); // Wrong magic
    }
}
