#!/bin/bash
set -e

cargo build --release -p growterm-app

APP="/Applications/growTerm.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/growterm "$APP/Contents/MacOS/growterm"
codesign --force --sign - "$APP/Contents/MacOS/growterm"
cp assets/icon.icns "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>growterm</string>
	<key>CFBundleIdentifier</key>
	<string>com.juniqlim.growterm</string>
	<key>CFBundleName</key>
	<string>growTerm</string>
	<key>CFBundleVersion</key>
	<string>0.1.0</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1.0</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
EOF

echo "Installed to $APP"
