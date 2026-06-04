# Snenk Bridge

A free, lightweight, open-source alternative to [VBridger](https://store.steampowered.com/app/1898830/VBridger/) for VTubers.

Snenk Bridge takes face tracking data from your iPhone, lets you transform it with custom math expressions, and sends it to [VTubeStudio](https://store.steampowered.com/app/1325860/VTube_Studio/) on your PC. You get full control over how your tracking data maps to your model's parameters no black boxes.

There's a built-in editor with a live preview, so you can shape the mapping and watch it move as you tweak before you even open VTube Studio. Runs on macOS, Windows, and Linux.

## Support

Snenk Bridge is free and I work on it in my spare time. If it's useful to you and you'd like to chip in, you can support it [here](https://github.com/sponsors/FaeyUmbrea). Completely optional it just helps me keep building.

## Supported Tracking Apps

- [VTubeStudio](https://apps.apple.com/app/vtube-studio/id1511435444) (use `vts` or `vtubestudio`)
- [iFacialMocap](https://apps.apple.com/app/ifacialmocap/id1489470545) / [iFacialMocapTr](https://apps.apple.com/app/ifacialmocaptr/id1520971310) (use `ifm` or `ifacialmocap`)

## Getting Started

### UI

1. Launch the app.
2. Pick a preset from the dropdown at the top or open the **Editor** tab to build your own mapping.
3. In the **SOURCE** row, enter your phone's IP, choose your tracking app, and hit **Connect**.
4. The **TARGET** row points at VTube Studio (the defaults usually just work) hit **Connect** there too.
5. Switch to the **Preview** tab to watch your tracking come through, and tune from there.

### CLI (optional)

The CLI isn't built by default see [Building from Source](#building-from-source) if you want it. Run `snenk_bridge` with the following arguments:

Run `snenk_bridge` with the following arguments:

| Argument                     | Example       | Description            |
| ------------------------------------------------- | -------------------- | ---------------------------------- |
| `-c <path>`, `--config <path>`          | `-c test.json`    | Path to your JSON config      |
| `-p <IP>`, `--phone-ip <IP>`           | `-p 192.168.0.174`  | Your phone's local IP address   |
| `-t <type>`, `--tracking-client <type>`      | `-t ifm`       | Which tracking app to use     |
| `-f <ms>`, `--face_search_timeout <ms>`      | `-f 3000`      | Face detection timeout       |
| `-d <ms>`, `--config-reload-delay <ms>`      | `-d 10000`      | How often to check for config changes |
| `-h`, `--help`                  |           | Show help             |
| `-V`, `--version`                 |           | Show version            |

## Configuration

The config file is where you define how tracking data gets transformed into VTubeStudio parameters. See the [configuration docs](docs/configuration.md) for the full reference, available inputs, and examples.

A working example config is included as [`test.json`](test.json).

## Building from Source

```bash
# Clone the repo
git clone https://github.com/FaeyUmbrea/SnenkBridge.git
cd SnenkBridge

# Build the UI app (the default)
cargo build --release

# Build the optional CLI instead
cargo build --release --package snenk_bridge
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Credits

This project is a fork of [SandoitchiBridge](https://github.com/an1by/SandoitchiBridge) by an1by.
