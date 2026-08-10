set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Show local tool versions and required Flatpak runtimes.
doctor:
    rustc --version
    cargo --version
    flutter --version
    dart --version
    flatpak --version
    flatpak info org.freedesktop.Sdk//25.08 >/dev/null
    flatpak info org.flatpak.Builder >/dev/null

# Fetch Flutter dependencies.
deps:
    cd client/flutter && flutter pub get

# Format Rust and Flutter code.
fmt:
    cargo fmt --all
    cd client/flutter && dart format lib test

# Check formatting without changing files.
fmt-check:
    cargo fmt --all -- --check
    cd client/flutter && dart format --output=none --set-exit-if-changed lib test

# Run Rust checks and Flutter analysis.
check: rust-check flutter-analyze

# Run Rust clippy and Flutter analysis.
lint: rust-clippy flutter-analyze

# Build Rust server and Flutter Linux app.
build: rust-build flutter-build

# Run the preferred test suite inside Flatpak.
test *args:
    scripts/test-in-flatpak.sh {{args}}

# Run all tests on the host machine.
test-host: rust-test flutter-test

# Run a fuller local verification pass.
ci: fmt-check lint test

# Build and install the Flatpak package, without running its in-sandbox tests.
package-build:
    scripts/build-flatpak-package.sh

# Build, install, and test the Flatpak package.
package-test:
    scripts/test-flatpak-package.sh

# Build the Rust workspace.
rust-build:
    cargo build --workspace

# Build the Rust workspace in release mode.
rust-build-release:
    cargo build --workspace --release

# Check the Rust workspace.
rust-check:
    cargo check --workspace --all-targets

# Run Rust clippy.
rust-clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run Rust tests on the host machine.
rust-test *args:
    cargo test --workspace {{args}}

# Run the server on the host machine.
server *args:
    cargo run -p syntax-bridge-server -- {{args}}

# Run the Flutter app on Linux.
app *args:
    cd client/flutter && flutter run -d linux {{args}}

# Analyze the Flutter project.
flutter-analyze:
    cd client/flutter && flutter analyze

# Build the Flutter Linux app.
flutter-build:
    cd client/flutter && flutter build linux

# Build the Flutter Linux app in release mode.
flutter-build-release:
    cd client/flutter && flutter build linux --release

# Run Flutter tests on the host machine.
flutter-test *args:
    cd client/flutter && flutter test {{args}}

# Capture Flutter UI screenshots as test artifacts.
flutter-screenshots *args:
    cd client/flutter && flutter test test/screenshot_capture_test.dart test/ui_screenshots_test.dart {{args}}

# Capture Flutter UI screenshots and generate a browsable gallery.
screenshots *args:
    just flutter-screenshots {{args}}
    @dir="client/flutter/build/test-screenshots"; html="$dir/index.html"; { printf '%s\n' '<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Syntax Bridge screenshots</title><style>body{font:16px sans-serif;margin:24px;background:#f6f7f8;color:#1d1b20}main{display:grid;gap:24px}figure{margin:0;padding:16px;background:white;border:1px solid #d7dadd;border-radius:8px}img{display:block;max-width:100%;height:auto;border:1px solid #d7dadd}figcaption{margin-bottom:12px;font-weight:600}</style></head><body><main><h1>Syntax Bridge screenshots</h1>'; for image in "$dir"/*.bmp "$dir"/*.png; do [ -e "$image" ] || continue; name="$(basename "$image")"; printf '<figure><figcaption>%s</figcaption><img src="%s" alt="%s"></figure>\n' "$name" "$name" "$name"; done; printf '%s\n' '</main></body></html>'; } > "$html"; printf 'Screenshot gallery: %s\n' "$html"

# Check, rebuild, repackage, reinstall, and run the Flatpak app (no tests; see `just ci`).
run *args: check package-build
    flatpak run dev.syntax_bridge.SyntaxBridge {{args}}

# Run the already installed Flatpak app, skipping the rebuild.
flatpak-run *args:
    flatpak run dev.syntax_bridge.SyntaxBridge {{args}}

# Remove Rust and Flutter build outputs.
clean:
    cargo clean
    cd client/flutter && flutter clean
