//! C++ → IR lowering. One adapter per input language, kept separate from
//! `emit::dart` (output side) — see AGENTS.md's boundary between "análise de
//! entrada" and "geração de saída".

pub mod cpp;
