//! Static validation of transpiled Dart output (US-9): runs the real Dart
//! toolchain against an already-written package and translates each
//! diagnostic back to the C++ declaration it came from — see
//! `dart::DartDiagnostic`'s doc comment for what "back to" means today
//! (whole top-level declaration, not per-statement).
//!
//! `dart` is the only target-language adapter today, same reasoning as
//! `crate::emit`'s own single `dart` submodule.

pub mod dart;
