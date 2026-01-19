use crate::color::ColorScheme;
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// ICMP ping mode (may require elevated privileges)
    Icmp,
    /// UDP client mode - sends pings to a rttui server
    UdpClient,
    /// UDP server mode - echoes ping packets back to clients
    UdpServer,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Icmp => write!(f, "ICMP"),
            Mode::UdpClient => write!(f, "UDP Client"),
            Mode::UdpServer => write!(f, "UDP Server"),
        }
    }
}

#[derive(Parser, Debug, Clone)]
#[command(name = "rttui")]
#[command(about = "A visual ping graph with true-color terminal support")]
#[command(version)]
pub struct Config {
    /// Target host (IP address or hostname). If not provided, settings dialog opens.
    #[arg()]
    pub host: Option<String>,

    /// Ping mode
    #[arg(short, long, value_enum, default_value = "icmp")]
    pub mode: Mode,

    /// Ping interval in milliseconds
    #[arg(short, long, default_value = "1000")]
    pub interval: u64,

    /// UDP port for client/server mode
    #[arg(short, long, default_value = "44444")]
    pub port: u16,

    /// Bind address for UDP server mode (e.g., 0.0.0.0, ::, 192.168.1.1)
    #[arg(long)]
    pub bind: Option<String>,

    /// Ping timeout in milliseconds
    #[arg(short, long, default_value = "3000")]
    pub timeout: u64,

    /// Color scale - RTT (ms) that is considered "bad"
    /// The gradient scales proportionally from low to this value
    #[arg(short = 's', long, default_value = "200")]
    pub scale: u64,

    /// Color scheme for the graph
    #[arg(short = 'c', long, value_enum, default_value = "dark")]
    pub colors: ColorScheme,

    /// Hide the terminal cursor while running
    #[arg(long, default_value = "false")]
    pub hide_cursor: bool,

    /// History buffer size in megabytes (approximate)
    #[arg(short = 'b', long, default_value = "10")]
    pub buffer_mb: u64,
}

impl Config {
    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        // For server mode, host is not required
        // For client modes without host, settings dialog will prompt for it

        if self.interval == 0 {
            anyhow::bail!("Interval must be greater than 0");
        }

        if self.timeout == 0 {
            anyhow::bail!("Timeout must be greater than 0");
        }

        if self.scale == 0 {
            anyhow::bail!("Scale must be greater than 0");
        }

        if self.buffer_mb == 0 {
            anyhow::bail!("Buffer size must be greater than 0");
        }

        Ok(())
    }

    /// Calculate max history entries from buffer size in MB
    /// Each PingResult is approximately 48 bytes
    pub fn max_history(&self) -> usize {
        const BYTES_PER_RESULT: usize = 48;
        let bytes = self.buffer_mb as usize * 1024 * 1024;
        bytes / BYTES_PER_RESULT
    }

    /// Get target as resolved IP address (for display)
    #[allow(dead_code)]
    pub fn target_display(&self) -> String {
        self.host
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", self.port))
    }
}
