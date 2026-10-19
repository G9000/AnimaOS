# Releasing Anima

Pushing a tag such as `v0.2.0` publishes a GitHub Release of the packaged `anima`
binary for macOS on Apple Silicon (`aarch64-apple-darwin`) and Windows x64
(`x86_64-pc-windows-msvc`). Only `hosts/anima` ships; `anima-daemon` and the rest of
the workspace are excluded.

The Release workflow runs for any pushed tag that looks like a version: dist's
trigger is the pattern `'**[0-9]+.[0-9]+.[0-9]+*'`, so `v0.2.0`, `0.2.0` and
`anima/0.2.0` all start a release, not only `v*` tags. Use `v<version>` tags, and
protect them (see the tag ruleset below).

| Asset | What it is |
| --- | --- |
| `anima-aarch64-apple-darwin.tar.gz` | macOS binary, code-signed as `dev.animaos.anima` |
| `anima-x86_64-pc-windows-msvc.zip` | Windows binary |
| `<archive>.sha256` | SHA-256 of each archive |
| `<archive>.minisig` | minisign signature of each archive, uploaded by the signing job |
| `sha256.sum` | All checksums in one file (dist always writes it) |
| `anima-installer.sh` | `curl … \| sh` installer for macOS |
| `anima-installer.ps1` | PowerShell installer for Windows |
| `dist-manifest.json` | dist's description of the release (dist always uploads it) |

The installers can't run commands after installing, so they end with
`Run 'anima setup' to finish installing Anima.`

## How the pipeline works

- `dist-workspace.toml` holds the configuration for [cargo-dist](https://opensource.axo.dev/cargo-dist/)
  **0.32.0**.
- `dist generate` writes `.github/workflows/release.yml` from that file. Never edit
  `release.yml` by hand. On every pull request the workflow runs `dist plan`, which
  fails if `release.yml` is out of date.
- `.github/build-setup.yml` runs in each build job before cargo. It installs Bun and
  builds the web console (`@animaOS-SWARM/sdk`, then `@animaOS-SWARM/web`).
  `anima` is built with `features = ["bundled-ui"]`, which embeds `apps/web/dist`.
- dist signs the macOS binary with the `CODESIGN_*` secrets. The binary carries an
  Info.plist (`hosts/anima/build.rs`), so its signing identifier is
  `dev.animaos.anima` and not one derived from the file name.
- `.github/build-setup.yml` installs with `bun install --ignore-scripts`, so no
  package's install script runs in a job that holds signing secrets, and it blanks
  the `CODESIGN_*` secrets and `GH_TOKEN` for its own steps. Its actions are pinned
  to commit SHAs.
- After the release is published, `.github/workflows/sign-release.yml` signs the
  archives. It takes them from this workflow run's own build artifacts
  (`artifacts-build-local-*`), not from the GitHub Release, whose assets can be
  replaced after upload. It signs them with `tools/release/sign-assets.sh`, checks
  each signature against the committed `hosts/anima/release-key.pub` (the job fails
  on a mismatch; while that file is still the placeholder the check is skipped and
  the log says so), then uploads the `.minisig` files. Until those files are
  uploaded, `anima update` treats the release as incomplete and doesn't offer it.

To change the configuration, install the same dist version, edit
`dist-workspace.toml`, regenerate and commit both files:

```sh
cargo install cargo-dist --version 0.32.0 --locked
dist generate        # rewrites .github/workflows/release.yml
dist plan            # lists what a release would publish
```

## One-time setup

### 1. The minisign release key

The updater installs an archive only if its minisign signature verifies against the
public key compiled into the running binary (`hosts/anima/release-key.pub`).

The committed file is a placeholder. Until the real public key replaces it,
`anima update` and the background check report "Updates are not configured for this
build". Updates are disabled by design until then. A binary built with the
placeholder can never update itself, so people on such a build reinstall with the
installer.

```sh
brew install minisign      # Windows: scoop install minisign
minisign -G -p anima-release.pub -s anima-release.key
```

Choose a strong password when asked. Then:

1. Replace `hosts/anima/release-key.pub` with `anima-release.pub` (both lines:
   `untrusted comment: …` and the key). Commit it before cutting the first release
   that should be able to update.
2. Keep `anima-release.key` and its password in your password manager and an offline
   backup. Releases signed with any other key are refused. Replacing the key means
   shipping a build that trusts the new key, so people still on older builds have to
   reinstall.

### 2. The code-signing certificate (macOS)

macOS keychain items remember which signed program may read them. A stable signing
identity keeps that permission across updates, so Anima doesn't ask for keychain
access again after each update. The certificate is self-signed: it gives a stable
identity, not Apple notarization.

1. Open Keychain Access → Keychain Access menu → Certificate Assistant → Create a
   Certificate…
2. Name: `AnimaOS Code Signing`. Identity Type: Self-Signed Root. Certificate Type:
   Code Signing. Create.
3. In **My Certificates**, right-click `AnimaOS Code Signing` → Export… → save as
   `AnimaOS-Code-Signing.p12` with an export password.
4. Base64-encode it for the secret: `base64 -i AnimaOS-Code-Signing.p12 | pbcopy`.

Keep the `.p12` and its password in your backups. Reuse this certificate for every
release. A new certificate changes the signature, and the keychain asks people to
approve access again.

### 3. Secrets

Set all of these before the first tag.

| Secret | Value | Where |
| --- | --- | --- |
| `CODESIGN_CERTIFICATE` | the base64 text of `AnimaOS-Code-Signing.p12` | repository |
| `CODESIGN_CERTIFICATE_PASSWORD` | the `.p12` export password | repository |
| `CODESIGN_IDENTITY` | `AnimaOS Code Signing` (the certificate's name) | repository |
| `MINISIGN_SECRET_KEY` | the full contents of `anima-release.key` (both lines) | `release` environment |
| `MINISIGN_PASSWORD` | the minisign key password | `release` environment |

The `CODESIGN_*` secrets go under Settings → Secrets and variables → Actions →
Repository secrets.

The signing job runs in a GitHub Environment named `release`. Create it under
Settings → Environments → New environment, add **Required reviewers** (yourself), and
under **Deployment branches and tags** allow only tags matching `v*`. Add the two
`MINISIGN_*` secrets there as environment secrets, not repository secrets. Then no
release is signed until a reviewer approves the job, and no other workflow or branch
can read the key.

### 4. A tag ruleset

Anyone who can push a version tag starts a release. Under Settings → Rules →
Rulesets → New tag ruleset, target all tags (dist's trigger matches any version-like
tag, not only `v*`), restrict creation, update and deletion to the maintainers who cut
releases, and block force pushes.

## The signature convention

Each archive gets a detached `<archive>.minisig`. It is a prehashed minisign
signature (the minisign default; legacy signatures are refused) over the exact
archive bytes. Its trusted comment must be exactly:

```text
anima {version} {target} file:{asset_name}
```

- `version` is the tag without its leading `v`, in canonical semver form (`0.2.0` for
  `v0.2.0`).
- `target` is the Rust target triple.
- `asset_name` is the archive's file name.

For example: `anima 0.2.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz`.

minisign covers the trusted comment with the signature, so a genuinely signed older
or other-platform archive can't be passed off as this release. The updater rebuilds
the comment from the release it found and refuses any mismatch. The format is
documented on `release_trusted_comment` in
[`hosts/anima/src/update/verify.rs`](../../hosts/anima/src/update/verify.rs).

`tools/release/sign-assets.sh <version> <secret-key-file> <archive>...` produces
exactly this, taking the target from the file name. To sign or re-sign by hand
(MINISIGN_PASSWORD is read from the environment and passed on stdin):

```sh
MINISIGN_PASSWORD=… tools/release/sign-assets.sh 0.2.0 anima-release.key \
  anima-aarch64-apple-darwin.tar.gz anima-x86_64-pc-windows-msvc.zip
gh release upload v0.2.0 anima-*.minisig --clobber
```

Check a signature with
`minisign -V -p hosts/anima/release-key.pub -m <archive> -x <archive>.minisig`. It
prints the trusted comment. The signing job runs exactly this for every archive
before it uploads the signatures.

The round-trip test signs with a throwaway key through the script and checks the
result with the updater's `verify`. It needs `minisign` on `PATH`, so it is ignored by
default:

```sh
cargo test -p anima sign_script -- --include-ignored
```

## Cutting a release

1. Set `version` in the root `Cargo.toml` under `[workspace.package]`, for example
   `0.2.0`. Then run `cargo check -p anima` to update `Cargo.lock`.
2. Commit: `git commit -am "release: v0.2.0"`, then push the commit.
3. Tag and push the tag: `git tag v0.2.0 && git push origin v0.2.0`.
4. Watch the **Release** workflow in GitHub Actions. The `custom-sign-release` job
   waits for a reviewer of the `release` environment: approve it. When it has
   finished, the release has both `.minisig` files.

The updater only offers stable releases (no `-suffix`) that are newer than the running
version and have their archive, `.sha256` and `.minisig` uploaded. A tag with a
suffix, such as `v0.3.0-rc.1`, becomes a GitHub prerelease and is never offered.

## Dry release (owner step, before the first real release)

Run this once after setting up the secrets, to prove the pipeline end to end. Use the
main repository: the installers download from the `repository` URL in
`hosts/anima/Cargo.toml`, so a fork's installer would fetch the main repository's
assets.

1. On a scratch branch, set `[workspace.package] version = "0.0.0-test.1"`, run
   `cargo check -p anima`, commit, then `git tag v0.0.0-test.1 && git push origin v0.0.0-test.1`.
2. When the workflow finishes, check that the prerelease has:
   - both archives, their `.sha256` files and both `.minisig` files;
   - signatures that pass `minisign -V -p hosts/anima/release-key.pub -m <archive>`,
     with trusted comments of the form `anima 0.0.0-test.1 <target> file:<archive>`.
3. Download `anima-aarch64-apple-darwin.tar.gz` and unpack it. In the output of
   `codesign -dv anima`, check that `Identifier=dev.animaos.anima` appears.
4. Run the shell installer from the release page:
   `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/G9000/animaOS-SWARM/releases/download/v0.0.0-test.1/anima-installer.sh | sh`.
   Check that `anima --version` prints `anima 0.0.0-test.1`.
5. Delete the prerelease and its tag (`git push origin :refs/tags/v0.0.0-test.1`),
   then the scratch branch.
