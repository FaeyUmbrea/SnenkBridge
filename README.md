# SnenkBridge

**An open-source, cross-platform face tracking bridge for VTubers.**

SnenkBridge takes facial tracking data from your iPhone, transforms it into Live2D parameters, and sends it to VTube Studio.

The existing tools in this space are largely closed-source and heavily tied to Windows. I wanted something different: **a tool whose implementation you can actually inspect, that doesn't treat macOS and Linux as afterthoughts, and that gives riggers control over what happens between tracking and the model.**

So I built one.

[![GitHub Sponsors](https://img.shields.io/badge/Sponsor-FaeyUmbrea-ea4aaa?style=flat\&logo=github-sponsors)](https://github.com/sponsors/FaeyUmbrea)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat)](#getting-started)
[![VTube Studio](https://img.shields.io/badge/VTube%20Studio-Compatible-orange?style=flat\&logo=steam)](https://store.steampowered.com/app/1325860/VTube_Studio/)
[![License](https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-green?style=flat)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-dea584?style=flat\&logo=rust)](https://www.rust-lang.org/)

---

## Why does this exist?

Face tracking shouldn't depend on which desktop operating system you happen to use.

A lot of the existing VTuber tracking ecosystem is closed-source, Windows-first, or both. That makes it difficult to understand what's actually happening to your tracking data, difficult to extend existing tools, and unnecessarily limiting for people who use macOS or Linux.

**SnenkBridge is my attempt to fix that.**

It's developed under the **Void Monster** name by **Faey Umbrea**, with a focus on making the tracking pipeline open, inspectable, configurable, and properly cross-platform.

It's also a deliberate VBridger alternative: not just something that happens to perform a similar job, but something intended to give people a genuinely open option.

---

## What it does

SnenkBridge sits between your face tracker and VTube Studio:

```text
iPhone
  │
  │  ARKit face tracking
  ▼
SnenkBridge
  │
  │  parameter mapping / math / smoothing
  ▼
VTube Studio
  │
  ▼
Your model
```

Instead of simply passing tracking values through, SnenkBridge gives you tools for shaping that data before it reaches your model.

### Live 3D face preview

Inspect your tracking data on an interactive 3D ARKit face mesh in real time.

You can see things like head movement, blinking, mouth shapes and facial expressions directly in the preview, making it much easier to understand what your tracker is actually sending.

### Visual parameter editor

Create and tune mappings with curves, limits, smoothing, delay buffers and mathematical expressions.

The editor is designed so that you can experiment with a mapping and immediately see what it does.

### Presets

Use included presets as a starting point, or load configurations shared by other users.

### Cross-platform

SnenkBridge is written in Rust and runs natively on:

* Windows
* macOS
* Linux

Cross-platform support isn't a secondary compatibility mode. **It's one of the reasons the project exists.**

### Open source

The implementation is available for anyone to inspect, modify, fork or contribute to.

No black box sitting between your face and your model.

---

## Supported tracking apps

| App                                                                        | Identifier             | Notes                                                       |
| -------------------------------------------------------------------------- | ---------------------- | ----------------------------------------------------------- |
| [VTube Studio (iOS)](https://apps.apple.com/app/vtube-studio/id1511435444) | `vts` / `vtubestudio`  | Recommended. Enable 3rd-party PC broadcast in VTube Studio. |
| [iFacialMocap](https://apps.apple.com/app/ifacialmocap/id1489470545)       | `ifm` / `ifacialmocap` | Standard ARKit face tracking over UDP.                      |
| [iFacialMocapTr](https://apps.apple.com/app/ifacialmocaptr/id1520971310)   | `ifm` / `ifacialmocap` | Lightweight alternative with a free trial.                  |

---

## Getting started

### Graphical interface

1. Launch SnenkBridge.
2. Select a preset, or open the **Editor** to build your own mappings.
3. Enter your iPhone's local IP address in the **SOURCE** row.
4. Select your tracking application and connect.
5. Connect VTube Studio in the **TARGET** row.
6. Open **Preview** and start moving.

That's enough to get a preset running.

The editor is there when you want to go deeper, not something you have to understand before using the application.

### CLI

SnenkBridge also provides a command-line interface for headless setups and automation:

```bash
snenk_bridge -c preset.json -p 192.168.1.100 -t vts
```

| Option                        | Example            | Description                                   |
| ----------------------------- | ------------------ | --------------------------------------------- |
| `-c`, `--config`              | `-c preset.json`   | Configuration file                            |
| `-p`, `--phone-ip`            | `-p 192.168.0.174` | iPhone IP address                             |
| `-t`, `--tracking-client`     | `-t ifm`           | Tracking protocol                             |
| `-f`, `--face_search_timeout` | `-f 3000`          | Face detection search timeout in milliseconds |
| `-d`, `--config-reload-delay` | `-d 10000`         | Configuration reload interval in milliseconds |
| `-h`, `--help`                | `-h`               | Show help                                     |
| `-V`, `--version`             | `-V`               | Show version                                  |

---

## Configuration

The mapping system is intended to work at two levels.

You can simply use a preset and never think about the underlying math.

Or you can build your own transformations using mappings, expressions, curves, smoothing and delay buffers.

Documentation:

* [Configuration Guide](docs/configuration.md)
* [Example Configuration](test.json)
* [Preset Format Reference](docs/formats/snek-v1.md)

---

## Migrating from VBridger

SnenkBridge can read and convert compatible existing preset formats, making it possible to bring existing configurations across rather than rebuilding them from scratch.

---

## Troubleshooting

### My phone won't connect

Make sure your PC and iPhone are on the same network and that your tracking application is broadcasting its data.

On Windows, check that the firewall isn't blocking SnenkBridge. A helper script is included in the repository.

### VTube Studio isn't receiving anything

Make sure VTube Studio is running and its API connection is enabled. The default target port is `8001`.

### I don't know anything about the math

That's fine.

The included presets are intended to work without touching the editor. The expression and mapping system exists for people who want more control.

---

## Building from source

You'll need [Rust and Cargo](https://www.rust-lang.org/tools/install).

```bash
git clone https://github.com/FaeyUmbrea/SnenkBridge.git
cd SnenkBridge

cargo build --release
```

To build the CLI explicitly:

```bash
cargo build --release --package snenk_bridge
```

---

## Contributing

Bug reports, pull requests and feature ideas are welcome.

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.

---

## About

SnenkBridge is made and maintained by **[Faey Umbrea](https://github.com/FaeyUmbrea)** as part of **Void Monster**.

It originally started as a fork of [SandoitchiBridge](https://github.com/an1by/SandoitchiBridge) by an1by, and has since grown into its own project.

I make software because sometimes the existing options annoy me enough that it's easier to build the thing myself.

SnenkBridge is one of those things.

### Support

SnenkBridge is free and open source.

If you find it useful and want to support continued development, you can do so through [GitHub Sponsors](https://github.com/sponsors/FaeyUmbrea).

---

## License

SnenkBridge is released under the **GNU General Public License v3.0 (GPL-3.0)**. See [LICENSE](LICENSE).
