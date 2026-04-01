#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

cargo build -r --workspace

mkdir -p "$SCRIPT_DIR/target/bundle"

OS="$(uname -s)"
case "$OS" in
    Darwin)
        APP_NAME="SnenkBridge"
        APP_BUNDLE="$SCRIPT_DIR/target/bundle/${APP_NAME}.app"
        CONTENTS="$APP_BUNDLE/Contents"
        MACOS_DIR="$CONTENTS/MacOS"
        RESOURCES="$CONTENTS/Resources"

        rm -rf "$APP_BUNDLE"
        mkdir -p "$MACOS_DIR" "$RESOURCES"

        # Copy binaries
        cp "$SCRIPT_DIR/target/release/snenk_bridge" "$MACOS_DIR/snenk_bridge"
        cp "$SCRIPT_DIR/target/release/snenk_bridge_ui" "$MACOS_DIR/snenk_bridge_ui"

        ICON_ENTRY=""

        # Create Info.plist
        cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>com.faeyumbrea.snenkbridge</string>
    <key>CFBundleVersion</key>
    <string>0.3.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.3.0</string>
    <key>CFBundleExecutable</key>
    <string>snenk_bridge_ui</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    ${ICON_ENTRY}
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

        # Ad-hoc codesign
        codesign --force --deep --sign - "$APP_BUNDLE"

        # Also copy standalone binaries
        cp "$SCRIPT_DIR/target/release/snenk_bridge" "$SCRIPT_DIR/target/bundle/snenk_bridge"
        cp "$SCRIPT_DIR/target/release/snenk_bridge_ui" "$SCRIPT_DIR/target/bundle/snenk_bridge_ui"
        cp "$SCRIPT_DIR/README.md" "$SCRIPT_DIR/target/bundle/README.md"

        echo "Built macOS app bundle: $APP_BUNDLE"
        echo "Standalone binaries also in: $SCRIPT_DIR/target/bundle/"
        ;;

    Linux)
        cp "$SCRIPT_DIR/target/release/snenk_bridge" "$SCRIPT_DIR/target/bundle/snenk_bridge"
        cp "$SCRIPT_DIR/target/release/snenk_bridge_ui" "$SCRIPT_DIR/target/bundle/snenk_bridge_ui"
        cp "$SCRIPT_DIR/README.md" "$SCRIPT_DIR/target/bundle/README.md"

        echo "Built Linux binaries in: $SCRIPT_DIR/target/bundle/"
        ;;

    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac
