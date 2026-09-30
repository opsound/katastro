#!/bin/bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "$0")/.." && pwd)"
bundle="${1:-$project_root/local/Katastro.app}"
if [[ -n "${KATASTRO_BINARY:-}" ]]; then
    binary="$KATASTRO_BINARY"
else
    cargo build --manifest-path "$project_root/Cargo.toml" --release --bin katastro
    binary="$project_root/target/release/katastro"
fi
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binary" "$bundle/Contents/MacOS/katastro"
chmod +x "$bundle/Contents/MacOS/katastro"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>katastro</string>
<key>CFBundleIdentifier</key><string>dev.katastro.review</string>
<key>CFBundleName</key><string>Katastro</string>
<key>CFBundleDisplayName</key><string>Katastro</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
printf 'App ready: %s\n' "$bundle"
