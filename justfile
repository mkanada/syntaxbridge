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

# Run the conversion examples corpus (examples/), on the host machine.
examples *args:
    cargo test -p syntax-bridge-server --test conversion_examples {{args}}

# Regenerate examples/*/expected/ goldens from the current transpiler output.
# Always review the diff before committing — the golden is a review tool,
# not the contract (dart analyze + the behavioral oracle are).
examples-bless *args:
    SYNTAX_BRIDGE_BLESS=1 cargo test -p syntax-bridge-server --test conversion_examples {{args}}

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
    cd client/flutter && flutter test test/screenshot_capture_test.dart test/screenshots/ {{args}}

# Capture Flutter UI screenshots and regenerate the docs/screenshots/ gallery
# that GitHub renders inline (see AGENTS.md: every new client screen/step
# needs a screenshot test feeding this gallery).
screenshots *args:
    just flutter-screenshots {{args}}
    cd client/flutter && dart run tool/generate_screenshot_gallery.dart build/test-screenshots ../../docs/screenshots
    @printf 'Screenshot gallery: docs/screenshots/README.md\n'

# Publish the current working tree's UI to a Gist, to check from a phone
# while a change is still in progress. Never touches the main repository.
screenshots-wip:
    scripts/publish-wip-screenshots.sh

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

# Stage everything, commit, and push.
publish message:
    git add .
    git commit -m "{{message}}"
    git push
