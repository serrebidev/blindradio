# Network catalog audit

**Goal:** Research every existing network and lofiradio.ru, restore missing live stations, preserve station IDs, and release the verified catalog.

**Scope:** Existing network data and tests, the Linux release copy-back fix, and the user's Seafile playlist. Public, stable streams only; preserve existing entries when a transient probe cannot establish retirement.

1. Search each network individually. Compare official channel lists and APIs with the catalog. Record sources, exclusions, measured stream quality, and before/after counts.
2. Merge the researched network objects into `crates/blindradio-core/data/networks.json`. Keep network order, station slugs, and history keys stable. Add LoFi Radio after verifying its live channels.
3. Update `crates/blindradio-core/tests/sources_networks.rs` for the audited rosters and representative missing stations. Run the focused tests and the live `check_networks` example; investigate failures individually.
4. Verify proposed lossless streams through the real core decoder. Compare final singleton networks against `C:\Users\admin\cloud\Seafile\My Library\radios.m3u`; add absent entries or upgrade inferior streams without changing unrelated entries. Back up the original playlist first.
5. Run the existing `Upload radios.m3u` scheduled task and verify its result and the remote file hash.
6. Include the narrow `tools/build_linux_remote.sh` copy-back fix and its native Windows scp verification. Add the audit evidence and the 0.2.4 changelog section, then commit.
7. Run `cargo xtask release 0.2.4` with the Windows build and signing environment. Monitor Linux, macOS, and publication; verify all five packages and five signatures, Linux hashes, and the updater's signed Windows download.
