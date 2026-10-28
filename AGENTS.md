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
- User guide: `doc/readme.md` (F1 in the app, converted at build time)

## Release

- `cargo xtask release <version>`: checks `main` is clean, runs the tests and clippy, bumps the
  workspace version, commits `chore: release <version>`, builds Windows, creates the draft
  release, tags and pushes, dispatches the macOS workflow, builds Linux over SSH, verifies the
  assets, then publishes. `--windows-only` skips the macOS and Linux legs; `--dry-run` stops
  before pushing anything.
- Windows builds here: `cargo xtask dist-windows` makes `target/dist/BlindRadio.zip` and
  `BlindRadio_setup.exe`, signing both with the owner's certificate (`signtool`), and with
  minisign when `MINISIGN_KEY` (and `MINISIGN_PASSWORD`) are set.
- macOS builds on GitHub Actions (`macos-release.yml`, arm64 only), Linux over SSH on
  `root@serrebiradio.com` in a container (`tools/build_linux_remote.sh`).
- The signing key: `cargo xtask minisign-key <dir>` writes `minisign.key` and `minisign.pub`.
  The secret goes in the repository secret `MINISIGN_KEY` and in Vaultwarden, never in git. The
  public half is `PUBLIC_KEY` in `crates/blindradio/src/update.rs`; changing it means the copies
  already installed can no longer update, so it takes a new release.
- Every asset is published with a `.minisig` beside it. `cargo xtask sign <file>…` signs any file.
- Never run a cloud release and a local one at the same time: both bump from the latest tag.
- Notes come from the `CHANGELOG.md` section for the version.
