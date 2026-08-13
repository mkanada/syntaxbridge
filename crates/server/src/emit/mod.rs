//! IR → output-language emission. `dart` is the only adapter today — see
//! AGENTS.md's boundary between "análise de entrada" and "geração de
//! saída": this module only ever consumes [`crate::ir`], never `libclang`
//! cursors directly.

pub mod dart;
