# BlindRadio

Screen-reader-first internet radio player in Rust: core crate plus wxDragon desktop app.

## Build and test
- Build: `cargo build`
- Test: `cargo test --workspace`

## Windows prerequisites
- MSVC Build Tools 2022, CMake, Ninja.
- LLVM (`winget install LLVM.LLVM`) for libclang, which wxdragon-sys bindgen needs. Set `LIBCLANG_PATH=C:\Program Files\LLVM\bin` if bindgen cannot find it.
- Run from a shell where `cl.exe` is found. Otherwise run `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"` first.

## Rules
- Dependencies are never pinned: use `version = "*"` and keep newest.
- Strings are terse. Error Display is the inner text only; the UI adds any prefix.
- Hard tabs (`rustfmt.toml`), edition 2024.

## Docs
- Spec: `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md`
- Plan: `docs/superpowers/plans/2026-10-26-blindradio-01-audio-engine.md`
