# Native installers and first launch

**Goal:** An ordinary user can install and open Anima on Windows, Linux, or macOS without terminal setup. Preserve the Rust runtime and browser console.

**Design:** Package the existing standalone binary with pinned cargo-packager tooling: Windows x64 NSIS setup EXE, Linux x64 Debian and AppImage, macOS Apple Silicon and Intel app/DMG. The application prepares only a safe default workspace and local host configuration before opening the existing onboarding. Provider credentials and login startup remain explicit choices. An installer must preserve personal state and must not forcibly kill active Anima work. Native packages use installer-based upgrades; signed raw-binary updates remain available only to existing portable builds.

**Workspace:** Existing checkout, implementation branch `g9000/native-installers`; preserve unrelated generated Astro edits. The user subsequently authorized committing this scope, merging it into `main`, and pushing to GitHub. No release publication, production signing keys, or credential changes.

## Work items

- [x] Add an absolute `ANIMA_HOME` storage override and use it in process tests, fixing Windows profile isolation.
- [x] Demonstrate failing first-launch tests, then bootstrap safe host config/tokens on an unconfigured no-argument launch. Keep explicit CLI `start` setup behavior; add visible desktop startup errors and a browser-suppression switch for tests.
- [x] Add Linux XDG login startup and disable binary self-replacement for native installer builds.
- [x] Add packaging configuration, assets, bounded installer lifecycle hooks, reproducible Nx package targets, and native CI jobs. Do not hand-edit cargo-dist's generated release workflow.
- [x] Build a real Windows installer, test installation/launch/reinstallation/uninstallation in temporary directories, verify shortcuts/uninstall registration, and confirm user data survives.
- [x] Run focused regression tests, full Anima host tests, the required Rust daemon target, packaging checks, and independent review. Record native Linux/macOS validation limits honestly. The required repository-wide Rust gate remains failed as recorded below.
- [x] Update installation/release docs and provide the concrete local installer artifact.

## Acceptance

- A fresh no-argument launch opens browser onboarding rather than exiting with `Run anima setup`.
- Repeated launch preserves settings, tokens, workspace contents, and provider configuration.
- A Windows setup wizard and uninstaller work; Linux and macOS have native packaging definitions and native-host build/validation jobs.
- Uninstall does not delete Anima's user data or a chosen workspace, and installer upgrades never silently terminate active work.
- Product sources remain reusable/host-separated. Tests use isolated paths rather than Windows environment-home assumptions.
- Local evidence is distinguished from CI definitions and from signing/provider acceptance.

## Local evidence and remaining gates

- `bun x nx run anima:package --skipNxCache` passed, including the SDK, web production build, native release binary, and NSIS compilation.
- Installer: `target/native-installers/anima_0.1.0_x64-setup.exe`, 25,173,242 bytes; SHA-256 `1A35A00D801AD2D6F63468C706800A601A92CCCF1324CB326D3672A589A98493`.
- Real production installer acceptance passed in `target/native-acceptance-1792491965173/installer-result.json` and `launch-result.json`: install into a path with spaces; Start menu/desktop shortcuts; no-argument launch; authenticated browser onboarding without JavaScript errors; refusal to replace a running application without terminating it; repeat launch; reinstallation; normal uninstaller invocation; removal of executable, shortcuts, and registrations. Profile settings, admin/control tokens, private data canary, and workspace canary survived byte for byte. Browser evidence is `installed-onboarding.png` in the same folder. The test installation was removed; personal application state was not reset or deleted.
- `anima:package-check` passed: 10 tests and 62 assertions. Real namespace-isolated NSIS regressions separately passed silent downgrade refusal, upgrade startup retention across an old uninstaller and changed directory, and genuine uninstall/data preservation.
- Portable `bundled-ui` unit validation passed: 134 tests, one ignored. Final `bun x nx run anima:test --args='--release --locked --features native-install -- --test-threads=4' --skipNxCache` passed: 179 tests, zero failures, one ignored (136 units, 22 CLI clients, five first-launch cases, one foreground run, four setup cases, 11 supervisor cases). See `target/anima-native-verified-tests.log`.
- Independent review found and led to fixes for alternate-profile login startup, stale portable updater handoff, inherited AppImage variables, silent downgrades, upgrade startup retention, and maintenance uninstall completion. Actual uninstall testing also led to conditional cleanup of the installer-owned remembered path.
- Pinned cargo-dist 0.32.0 generated `release.yml` from `dist-workspace.toml`; `dist generate --check` passed. Both workflows parsed successfully. PR/manual runs produce artifacts; release-plan calls publish only this run's assets after all four native builds succeed. No release was published and no signing keys were configured.
- Required `bun x nx run rust-daemon:test --skipNxCache` was attempted and failed in its existing `core-rust:test` dependency: 10 `anima-worktree` Windows Git path failures (`//?/C:/...`, invalid argument). See `target/rust-daemon-installer-validation.log`. Daemon-only debug validation then encountered an existing missing `ort_sys` rlib artifact; the optional optimized full-daemon recompilation was stopped after six minutes to release the build lock. These are not passing repository-wide gates.
- `anima:lint` encountered formatting differences in untouched daemon/core sources. Scoped rustfmt and diff checks passed for changed host/release sources. Unrelated generated Astro changes were preserved.
- Linux x64 Debian/AppImage and macOS arm64/x64 app/DMG definitions and native-host first-launch build/test jobs are present; they have not run here. Existing WSL Ubuntu is newer than the release baseline and lacks Bun and packaging prerequisites. Graphical Linux/macOS installation and launch acceptance remain open.
- Artifacts are unsigned. Trusted Windows Authenticode signing and macOS Developer ID signing/notarization remain public-distribution gates. Provider/model/chat integrations were not exercised by installer tests.
