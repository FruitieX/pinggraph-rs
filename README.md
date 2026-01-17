# pinggraph-rs

A visual ping graph CLI tool with 256-color terminal support, built in Rust.

⚠️ Note on AI usage: Claude Opus 4.5 was used during development of this project.

Largely inspired by the excellent https://pinggraph.io/ tool.

![License](https://img.shields.io/badge/license-MIT-blue)

<img width="1603" height="791" alt="image" src="https://github.com/user-attachments/assets/2d3ca6a7-a674-4307-b5d1-8754703399e8" />

## Features

- **Real-time ping visualization** — Watch latency as a scrolling color-coded graph
- **ICMP & UDP modes** — Native ICMP ping (requires privileges) or UDP fallback
- **10 color schemes** — Classic, Dark, Ocean, Fire, Neon, Grayscale, Matrix, Plasma, Ice, Thermal
- **Interactive controls** — Pause/resume, scrollback history, mouse tooltips
- **Configurable settings** — Target, interval, scale, colors all adjustable at runtime
- **Statistics display** — Min/avg/max RTT, packet loss, jitter, sparkline graph

## Installation

### From Source

```bash
git clone https://github.com/FruitieX/pinggraph-rs
cd pinggraph-rs
cargo build --release
```

The binary will be at `target/release/pinggraph` (or `pinggraph.exe` on Windows).

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/FruitieX/pinggraph-rs/releases).

## Usage

```bash
# Basic usage (ICMP requires setcap cap_net_admin,cap_net_raw capabilities or root/admin privileges)
pinggraph google.com

# UDP mode (no special privileges needed)
pinggraph -u 1.1.1.1 -p 53

# Custom interval and scale
pinggraph -i 500 -s 200 8.8.8.8

# With specific color scheme
pinggraph -c ocean cloudflare.com
```

### Options

| Flag | Long | Description | Default |
|------|------|-------------|---------|
| `-u` | `--udp` | Use UDP mode | ICMP |
| `-p` | `--port <PORT>` | UDP target port | 53 |
| `-i` | `--interval <MS>` | Ping interval in milliseconds | 1000 |
| `-s` | `--scale <MS>` | Max RTT scale for graph | 100 |
| `-c` | `--colors <SCHEME>` | Color scheme | dark |
| | `--hide-cursor` | Hide the graph cursor | false |

### Controls

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `Space` | Pause/Resume |
| `↑` / `↓` | Scroll through history |
| `Home` / `End` | Jump to start/end |
| `s` | Open settings menu |
| `Mouse click` | Show ping details tooltip |

## Requirements

- **ICMP mode**: Requires setcap cap_net_admin,cap_net_raw capabilities or administrator/root privileges
- **UDP mode**: No special privileges needed
- Terminal with 256-color support recommended

## License

MIT
