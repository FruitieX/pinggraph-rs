# rttui

A visual ping graph CLI tool with 256-color terminal support, using [Ratatui](https://ratatui.rs/).

⚠️ Note on AI usage: Claude Opus 4.5 was used during development of this project.

Largely inspired by the excellent https://pinggraph.io/ tool.

![License](https://img.shields.io/badge/license-MIT-blue)

<img width="1603" height="791" alt="image" src="https://github.com/user-attachments/assets/2d3ca6a7-a674-4307-b5d1-8754703399e8" />

## Features

- **Real-time ping visualization** — Watch latency as a scrolling color-coded graph
- **ICMP & UDP modes** — Native ICMP ping or UDP client/server mode
- **Interactive controls** — Pause/resume, scrollback history, mouse tooltips
- **Configurable settings** — Target, interval, scale, color scheme adjustable at runtime
- **Statistics display** — Min/avg/max RTT, packet loss, jitter, sparkline graph

## Installation

### From Source

```bash
git clone https://github.com/FruitieX/rttui
cd rttui
cargo build --release
```

The binary will be at `target/release/rttui` (or `rttui.exe` on Windows).

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/FruitieX/rttui/releases).

## Usage

```bash
# Basic usage (see Requirements section below for ICMP mode setup on Linux)
rttui google.com

# UDP mode
rttui -m udp-server -p 1234
rttui -m udp-client -p 1234 [HOST]

# Custom interval and scale
rttui -i 500 -s 200 8.8.8.8

# With specific color scheme
rttui -c ocean cloudflare.com
```

### Options

```
Usage: rttui [OPTIONS] [HOST]

Arguments:
  [HOST]  Target host (IP address or hostname). If not provided, settings dialog opens

Options:
  -m, --mode <MODE>            Ping mode [default: icmp] [possible values: icmp, udp-client, udp-server]
  -i, --interval <INTERVAL>    Ping interval in milliseconds [default: 1000]
  -p, --port <PORT>            UDP port for client/server mode [default: 44444]
      --bind <BIND>            Bind address for UDP server mode (e.g., 0.0.0.0, ::, 192.168.1.1)
  -t, --timeout <TIMEOUT>      Ping timeout in milliseconds [default: 3000]
  -s, --scale <SCALE>          Color scale - RTT (ms) that is considered "bad" The gradient scales proportionally from low to this value [default: 200]
  -c, --colors <COLORS>        Color scheme for the graph [default: dark] [possible values: classic, dark, ocean, fire, neon, grayscale, matrix, plasma, ice, thermal]
      --hide-cursor            Hide the terminal cursor while running
  -b, --buffer-mb <BUFFER_MB>  History buffer size in megabytes (approximate) [default: 10]
  -h, --help                   Print help (see more with '--help')
  -V, --version                Print version
```

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

- Terminal with 256-color support recommended

### ICMP Mode on Linux

On Linux, ICMP mode uses unprivileged ICMP datagram sockets (not raw sockets). This is controlled by the `net.ipv4.ping_group_range` kernel parameter, which specifies the range of group IDs allowed to create these sockets.

**Check if ICMP sockets are enabled:**

```bash
$ sysctl net.ipv4.ping_group_range
net.ipv4.ping_group_range = 1	0
```

If the output shows `1  0` (min > max), ICMP datagram sockets are **disabled** for all users. This appears to be the default on Ubuntu and some other distributions.

**Enable temporarily (until next reboot):**

```bash
sudo sysctl -w net.ipv4.ping_group_range="$(printf '0\t10000')"
```

This allows users with GIDs 0–10000 to use ICMP sockets.

**Enable permanently:**

```bash
sudo sh -c "printf 'net.ipv4.ping_group_range=0\t10000\n' >> /etc/sysctl.conf"
```

See [ICMP Sockets on Linux](https://ekman.cx/articles/icmp_sockets/#linux) for more details.

### UDP Mode

UDP mode requires no special privileges and works out of the box, but requires a rttui UDP server running on the target.

### Windows

On Windows, ICMP mode works without additional configuration.

## License

MIT
