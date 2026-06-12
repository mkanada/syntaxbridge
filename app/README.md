# Syntax Bridge App

Flutter desktop frontend for Syntax Bridge.

The app calls the Rust core through `flutter_rust_bridge` and shows diagnostics for the conversion environment. The project is currently focused on Linux desktop and Flatpak packaging.

## Development

Common checks from this directory:

```sh
flutter test
flutter test integration_test/simple_test.dart -d linux
```

Rust checks live under `rust/`:

```sh
cargo test
```
