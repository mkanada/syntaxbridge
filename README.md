# Syntax Bridge

Syntax Bridge is an early-stage desktop tool for supporting incremental conversion of C++ codebases to Dart. The goal is not to perform a blind automatic translation, but to guide a validated, traceable migration process where ambiguous decisions are surfaced to the user and stored for reuse.

The first real-world target used to drive development is `verovio`. This keeps the project focused on practical C++ conversion cases instead of trying to support the entire C++ language upfront.

## Project Goals

- Import C++ projects or individual C++ files.
- Parse the visual structure of C++ code with Tree-sitter, preserving comments and source layout as much as possible, and use libclang for semantic information such as symbols, types and dependencies.
- Generate equivalent Dart code incrementally.
- Ask the user for decisions when conversion rules are ambiguous.
- Persist mapping and conversion decisions in SQLite.
- Validate generated Dart through compilation and tests.
- Track converted, partially converted and unsupported items.

## Architecture

Syntax Bridge is planned as a Flutter desktop application backed by a Rust core.

- `app/`: Flutter desktop application.
- `app/rust_builder/`: Rust integration used by the Flutter app through `flutter_rust_bridge`.
- `packaging/flatpak/`: Flatpak packaging files.
- `docs/`: planning notes and development steps.
- `tmp/`: exploratory or reference material, including previous conversion experiments.

The Rust core is expected to handle parsing, analysis, persistence, conversion rules, code generation and integration with external tooling. The Flutter UI is responsible for project navigation, diagnostics, conversion workflow guidance and user decisions.

## Conversion Approach

Syntax Bridge intentionally avoids runtime AI. Conversions should be based on explicit rules, parsers, persisted mappings, validation tools and human review.

The intended workflow is:

1. Check and prepare the embedded tooling environment.
2. Import a C++ project.
3. Build tests that capture the behavior of the original C++ code.
4. Map symbols, types and ownership patterns.
5. Generate a compilable Dart project structure.
6. Convert functions, methods and classes incrementally.
7. Compile and test after each meaningful step.
8. Persist decisions so interrupted conversions can be resumed.

## Tooling Direction

Planned or expected tools include Tree-sitter, CMake, Clang/libclang, the Dart analysis server, gtest, KLEE and SQLite.

Development follows a TDD workflow: start with a failing test, implement the smallest correct change, then run the test again and confirm it passes.

## Flatpak

The official Flatpak application ID is:

```text
io.github.mkanada.syntaxbridge
```

The previous local installation IDs, `io.github.syntaxbridge.SyntaxBridge` and `com.syntaxbridge.SyntaxBridge`, are obsolete and should not be used as the primary application ID.

## Current Status

The project is in its initial development phase. The current baseline includes a Flutter desktop app calling Rust through `flutter_rust_bridge`, plus Flatpak packaging work. The next major direction is defining embedded tooling and verification tests for the conversion environment.
