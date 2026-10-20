# Install Anima

Anima runs locally and opens its console in your browser. The desktop packages
include the Rust host and the production console. Rust and Bun are needed only
when building from source.

| Computer | Package | Installation |
| --- | --- | --- |
| Windows x64 | `anima_<version>_x64-setup.exe` | Run the setup wizard, then open **Anima** from Start. Installs for your Windows account without administrator permission. |
| Linux x64, glibc 2.35 or newer | `.deb` | Open with your package manager, or use `sudo apt install ./<package>.deb`. Open **Anima** from the application menu. |
| Linux x64, glibc 2.35 or newer | `.AppImage` | Keep it in a permanent folder, allow execution (`chmod +x <file>.AppImage`), and open it. AppImage support may require your distribution's FUSE compatibility package or extraction mode. |
| macOS Apple Silicon or Intel | Matching architecture `.dmg` | Open the disk image, drag **Anima** to Applications, eject the image, and open Anima. |

A graphical desktop and browser are required. Linux currently has no tray icon.
Windows ARM and Linux ARM packages are not configured in this release pipeline.

These installers are under development. The Windows installer is exercised locally;
Linux and macOS build jobs are defined, with native installation acceptance still
required on those systems. The current artifacts are unsigned. Public Windows
distribution still needs trusted Authenticode signing, and public macOS
distribution needs Developer ID signing and notarization. An unsigned macOS app
may be blocked by Gatekeeper.

## First launch

Open Anima normally. It prepares a local `anima-workspace` folder in your home
directory and private host settings under `~/.animaos-swarm`, starts the local
runtime, and opens browser onboarding. Choose your companion, workspace access,
and model provider there. A provider connection is required before model work can
run. Setup does not import shell credentials or turn on login startup.

Opening Anima again reuses the same runtime and saved configuration. If startup
fails, a desktop error dialog explains the failure; your workspace is preserved.
Settings, credentials, logs, and runtime data remain local to your account. Treat
the private host folder and your chosen workspace as personal data when backing up.

## Updates and removal

Finish active work and quit Anima before installing a newer package of the same
format. The Windows installer refuses to continue while Anima is running, including
silent installs. It retains an enabled login startup preference during upgrades.
Windows version downgrades are refused.

Native installations use package upgrades. The portable binary's `anima update`
command cannot replace an installer-managed executable. Native installation does
not provide automatic rollback.

On Windows, remove Anima through **Installed apps**. On Debian-based Linux, use
your package manager. For an AppImage or macOS app, remove the application file.
Before removing a Linux or macOS application, turn off **Start at login** in Anima.
Removing the program preserves `~/.animaos-swarm`, your credentials, and all chosen
workspaces. Back them up separately if you intend to move computers.

## Build from source

Follow [Native desktop installers](../../tools/release/README.md#native-desktop-installers)
for the pinned packaging tool, build prerequisites, and `bun x nx run anima:package`.
Packages appear in `target/native-installers/`. Release jobs build each package on
its native operating system; a Windows machine does not validate Linux/macOS apps.

For isolated testing, the absolute `ANIMA_HOME` environment variable selects an
alternate profile home. Its private settings live at
`ANIMA_HOME/.animaos-swarm`, and its default workspace is
`ANIMA_HOME/anima-workspace`. This profile inherits through child processes.
Login startup changes are disabled for alternate profiles; launch them explicitly.
Ordinary installed use does not need this variable.
