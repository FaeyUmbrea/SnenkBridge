# Snenk Bridge

A free, lightweight, open-source alternative to [VBridger](https://store.steampowered.com/app/1898830/VBridger/) for VTubers.

Snenk Bridge takes face tracking data from your iPhone, lets you transform it with custom math expressions, and sends it to [VTubeStudio](https://store.steampowered.com/app/1325860/VTube_Studio/) on your PC. You get full control over how your tracking data maps to your model's parameters — no black boxes.

## Supported Tracking Apps

- [VTubeStudio](https://apps.apple.com/app/vtube-studio/id1511435444) (use `vts` or `vtubestudio`)
- [iFacialMocap](https://apps.apple.com/app/ifacialmocap/id1489470545) / [iFacialMocapTr](https://apps.apple.com/app/ifacialmocaptr/id1520971310) (use `ifm` or `ifacialmocap`)

## Getting Started

### UI

1. Launch `snenk_bridge_ui`
2. Set the path to your config file (type it in or use the browse button)
3. Enter your phone's local IP address
4. Pick your tracking app
5. Set the face found timeout (in milliseconds) — this controls how long it waits before assuming the face is gone
6. Hit **Connect**
7. You can close the window after that — it keeps running in the background

> [!TIP]
> Use the system tray icon to show the window again or exit the app.

### CLI

Run `snenk_bridge` with the following arguments:

| Argument                                          | Example              | Description                        |
| ------------------------------------------------- | -------------------- | ---------------------------------- |
| `-c <path>`, `--config <path>`                    | `-c test.json`       | Path to your JSON config           |
| `-p <IP>`, `--phone-ip <IP>`                      | `-p 192.168.0.174`   | Your phone's local IP address      |
| `-t <type>`, `--tracking-client <type>`            | `-t ifm`             | Which tracking app to use          |
| `-f <ms>`, `--face_search_timeout <ms>`           | `-f 3000`            | Face detection timeout             |
| `-d <ms>`, `--config-reload-delay <ms>`           | `-d 10000`           | How often to check for config changes |
| `-h`, `--help`                                    |                      | Show help                          |
| `-V`, `--version`                                 |                      | Show version                       |

## Configuration

The config file is where you define how tracking data gets transformed into VTubeStudio parameters. See the [configuration docs](docs/configuration.md) for the full reference, available inputs, and examples.

A working example config is included as [`test.json`](test.json).

## Building from Source

```bash
# Clone the repo
git clone https://github.com/FaeyUmbrea/SnenkBridge.git
cd SnenkBridge

# Build both CLI and UI
cargo build --release

# Or just one of them
cargo build --release --package snenk_bridge      # CLI only
cargo build --release --package snenk_bridge_ui   # UI only
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Credits

This project is a fork of [SandoitchiBridge](https://github.com/an1by/SandoitchiBridge) by an1by.
