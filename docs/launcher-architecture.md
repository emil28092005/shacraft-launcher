# ShaCraft Launcher architecture

## Current capability

The launcher persists local settings, synchronises Aeronautics mod/config
files from the signed ShaCraft v2 manifest, installs the exact Minecraft +
NeoForge version the manifest specifies, and launches the game. A player
signs in with the same local ShaCraft account used on the website. The game
identity for ordinary Play is derived only from that account's verified Aeronautics nickname;
the legacy editable nickname setting is not trusted at launch.

The interface also shows a live Aeronautics player count from the fixed,
read-only `https://shacraft.ru/api/online/aoc` endpoint. It is display-only:
the result never controls files, versions, URLs, or the launch command.

The launcher also checks signed GitHub releases for its own updates. AppImage,
Windows x64 installers and native Intel/Apple Silicon macOS app bundles use
explicit install-and-restart; Debian packages use manual package management.
Production publication and OS signing/notarization require operator setup;
see `updater-release.md`. Version 0.1.1 has no updater and must be upgraded
manually once. User-selectable profile directories and managed-only reset
remain unimplemented.

## Data flow

One verified ShaCraft snapshot is fetched inside the installation lock for
each Play/Repair operation. Every stage, the displayed completion metadata and
the spawn use that snapshot; a newly published manifest is used next time.
Two independent trust pipelines feed one launch:

```text
ShaCraft manifest (mods/config + which MC/loader/Java version to use)
  signed-manifest endpoint -> Ed25519 verification (remote.rs)
  -> manifest schema + URL/path validation (manifest.rs)
  -> inventory reconciliation, verified staging, durable apply journal (profile.rs)

Game itself (never controlled by the manifest above)
  Mojang version manifest -> SHA-1-verified version JSON (mojang.rs)
  -> Java 21 via Adoptium if none installed (runtime.rs)
  -> NeoForge's own installer, run headlessly (neoforge.rs)
  -> generic inheritsFrom merge of the two version JSONs (mojang.rs)
  -> SHA-1-verified merged libraries + platform natives (mojang.rs)
  -> verified ShaCraft account link (shacraft_account.rs)
  -> deterministic offline UUID for the linked nickname (session.rs)
  -> java process spawned with the merged classpath/args (launch.rs)
```

Profiles (ShaCraft-managed mods/config, and the player's own worlds/
screenshots/resourcepacks) live below Tauri's `app_data_dir()/profiles/
<profile-id>` — this becomes `--gameDir`. The shared vanilla+NeoForge
install (versions/libraries/assets/runtime, reused across profiles that
target the same Minecraft version) lives at `app_data_dir()/game`. Settings
live at `app_data_dir()/settings.json`, and the revocable ShaCraft session at
`app_data_dir()/shacraft-session` (mode 600 on Unix). Passwords are never
written to disk. None of these should be assumed to
be the system `.minecraft` directory.

## Aeronautics contract

- Profile ID: `aeronautics`
- Manifest endpoint:
  `https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest`
- Payload: manifest schema v1; also carries `minecraft.{version, loader,
  javaMajor}` (currently 1.21.1, NeoForge 21.1.248, Java 21) — the launcher
  reads this rather than hardcoding it, so a server-side version bump needs
  no launcher release.
- ShaCraft download files: HTTPS only, exact hosts `shacraft.ru` and
  `cdn.shacraft.ru`.
- Account API origin: fixed `https://shacraft.ru`; redirects are rejected.
- Launch identity: the most recently verified `aoc` nickname returned by the
  authenticated account API. Local nickname edits cannot select an identity.

## Planned but not implemented

1. User-selectable profile directory and structured launcher logs.
2. "Reset managed files only" recovery action that doesn't touch player
   worlds/screenshots/resourcepacks.
3. Production release credentials/protected environments and OS beta validation.
4. Cancellation, structured logs and a full cold-install/recovery beta on
   every target OS. Install progress reports bytes or installer work counts
   depending on the stage; these units are not interchangeable.

Do not represent these as completed features in UI or release notes.


## Module boundaries (2026-09-09)

React entrypoint → App/components → hooks → typed native service. Pure
reducers own game and profile states; IPC failures retain their real message.
Settings writes are serialized, and account restoration is single-flight
even under React StrictMode. A failed repair invalidates profile readiness.
Game exit may arrive before launch acknowledgement; the reducer handles both.
Browser preview cannot install/launch and does not simulate download progress.

Rust `lib.rs` registers commands from `commands/`. Installation and account
permits in `operations.rs` stay owned by blocking workers until completion.
ShaCraft sessions have a separate gate from the retained Microsoft module.
An additional OS file lock in `installation_lock.rs` covers all profiles and
the shared game installation across processes. It remains held by the child
watcher; a durable PID/start-time lease also protects a game that outlives its
launcher. This is exclusion, not cancellation.
`storage.rs` provides unique temporary files and atomic replacement; Unix
session files are created owner-only. Windows keeps a recoverable replacement
fallback if the OS refuses direct replacement. `trusted_http.rs` constrains provider
URLs and redirects. Manifest profile identity, size, signature, portable
paths and existing symlinks are checked before managed file writes.
Hostile same-user TOCTOU is outside this protection; it is not an OS sandbox.

## Verification and distribution

`npm test` covers asynchronous helpers and state transitions;
`npm run build` runs strict TypeScript before Vite. `cargo test --locked`
covers native policy and storage. Push/PR CI repeats checks on Linux.
The package workflow builds and checks Windows x64, Linux x64 and both macOS
architectures with disposable test signing keys. These artifacts cannot be
published as production updater releases. Separate protected workflows assemble
a complete signed release as a draft, re-verify its assets and only then publish.
Native cold-install, real self-update and launch tests are required before
calling a platform release-ready. CI package builds do not prove those flows.


## Reconciliation and recovery

`profiles/.<profile-id>.shacraft-state/` is outside the payload root. It stores
`inventory.json`, `pending.json`, transaction staging/backups/receipts and
explicit legacy-mod backups. The inventory records only files the launcher
actually wrote, with size/hash/policy and signed snapshot digest. An existing
file that already matches is usable, but is not silently claimed as owned.
Seed files are never owned for later removal.

All required downloads are staged and verified before apply. A write-ahead
journal records old/new states. Interrupted apply resumes by checking hashes;
readiness stays false while a journal, stale owned files or conflicts remain.
Old owned files are moved to a backup only when their current contents still
match the recorded version. Changed files remain conflicts; unrelated files
and player worlds, screenshots, options and resourcepacks are preserved.
A manifest is not authority to overwrite an unknown colliding local file.

“Разобрать моды” lists legacy/changed JAR candidates without selecting any.
The user explicitly chooses paths; native code rechecks each selected hash
and safe path before moving it to a recoverable backup with a receipt. Unknown
extra mods are informational, not automatically removed or adopted. Readiness
means the declared pack files were checked; it does not certify arbitrary
additional user mods. Afterwards rerun Repair or Play.

`game/cache/neoforge-receipts/<version>.json` records provenance for generated
NeoForge outputs. The verified installer recipe and its embedded version JSON
scope which files can be rebuilt. Missing/invalid receipts, corrupt JSON or a
mismatched generated JAR cause a clean isolated rebuild. No local legacy hash
is accepted as the first baseline. See `game-trust-boundary.md`.

## First entry and account proof v1

The account settings offer “Установить и войти для подтверждения” separately
from ordinary Play. The launcher prepares the exact signed pack first, then
requests and validates an authenticated grant at the fixed ShaCraft origin:
`POST /api/launcher/onboarding/start` and `/validate`. It requires an approved
managed `mods/shacraft-game-bridge-*.jar` entry in that signed snapshot. Until
that payload and server bridge are deployed, this action fails with an explicit
message; ordinary linked Play keeps its existing account gate.

The grant binds account, `aoc`, exact nickname/offline UUID, challenge and TTL
(10 minutes). Only the game child gets `SHACRAFT_ONBOARDING_TOKEN` in its
environment; normal launches remove inherited tokens. The frontend receives
only the explicit proof challenge. The client bridge sends the token once to
the fixed Aeronautics socket. The server bridge holds that connection until
LoginSystem succeeds and the player enters `/shacraft link <id> <code>`.
The bridge uses a server-only bearer secret for fixed HTTPS callbacks. A grant
is not a whitelist grant, password replacement or completed web link.

New nicknames keep the established operator-whitelist → first `/register`
policy; registered names must use their existing `/login` password. Prism can
still perform the explicit proof command without a launcher grant. Existing
account links stay valid with legacy provenance and optional re-verification;
no bulk revocation occurs. GET link status is read-only. Backend implementation,
migrations, game adapter and rollout procedure are in the companion server
repository, `docs/account-proof-v1.md` and `docs/access-delivery.md`.

## Interrupted spawn recovery

`installation-state/writer.lock` is an OS lock file, not a stale-lock sentinel;
never delete it while any launcher/game is running. `game-lease.json` records
`Starting` before spawn and `Running` with PID/start time before releasing the
worker. A known exited child is cleared automatically. A crash in the narrow
spawn/record interval or malformed lease fails closed because the child cannot
be proven absent.

For that explicit error only: close all ShaCraft Launcher and Minecraft
processes (or reboot), verify none remain, and rename `game-lease.json` to a
backup outside `installation-state`. Then open one launcher and run Repair.
Do not remove game/profile trees or an active lock file as a recovery shortcut.
The application data root is shown in settings; it is not system `.minecraft`.

## Local live preparation check

`cargo test --locked --manifest-path src-tauri/Cargo.toml
commands::game::tests::live_cold_install_and_corruption_repair -- --ignored --nocapture`
uses a fresh temporary application directory, real signed/provider downloads
and the official installer. It corrupts generated NeoForge JSON/JAR, repairs
both and repeats a healthy check. It never logs into a game server. It requires
network access and sufficient disk space; the printed directory is retained
for diagnosis. This does not replace Tauri IPC, graphical gameplay or the
client/server proof matrix on each supported OS.


## Launcher self-update boundary (0.2.0)

`updater/protocol.rs` pins `github.com/emil28092005/shacraft-launcher/releases`.
The exact raw `latest.json` response is verified using its detached minisign
signature and the committed updater public key before versions, URLs or notes
are parsed. This key is independent of ShaCraft's Ed25519 mod manifest. Signed
metadata binds a stable version and tag to an exact set of four platform and
four manual package descriptors; every descriptor has an exact repository/tag/
filename, size, SHA-256 and Tauri signature. HTTPS redirects are restricted to
that repository and GitHub's release asset CDN. Stable downgrades, unknown
platforms, missing signatures and incomplete metadata fail closed.

Only native commands check, download, install and open the fixed releases page.
The webview supplies no URLs, public keys, executable arguments, release version
or arbitrary file path. No generic updater plugin permission is granted to it.
The packaged native architecture selects the artifact: Windows preserves MSI
versus NSIS, macOS preserves Intel versus Apple Silicon, Linux only replaces an
AppImage. A Debian installation requires the user's package manager.

Checks run once per application UI lifecycle and on explicit request. They do
not install automatically. The settings drawer shows installed/available
versions, plain-text notes, progress, actionable errors and retry. The explicit
install button includes restart. UI session recovery codes, unsaved/failed
settings and account/game operations inhibit that action; native permits are
the final authority for concurrent writes. Preferences, sessions and game data
live outside the executable and are not migrated or erased by the updater.

`launcher-state/instance.lock` is a process-lifetime OS lock, preventing an idle
second cooperating launcher from retaining old code during replacement. All
native account/settings/game writes hold shared lifecycle permits. Replacement
holds the exclusive permit and `installation-state/writer.lock`, which also
checks a game process's durable PID/start-time lease. A normal window close is
inhibited during download/install. Worker permits survive a dropped IPC future.
Before invoking the platform installer, `pending-update.json` records current
and target versions. The Windows plugin hands off and exits; the marker remains.
Only startup of the exact target version clears it. Old versions and indeterminate
installer failures block native mutations and direct the user to manual recovery.
A corrupt marker opens recovery diagnostics and latches the mutation/launch ban
until application restart, even if that file is removed while the UI is open.
No guessed installer timeout
releases the gate. This cannot retroactively make old 0.1.1 binaries cooperate
with these locks.

First metadata/signature and artifact reads are bounded. Tauri updater 2.11.0
requires its own second check to construct a private installer context; that
check uses the fixed version endpoint, HTTPS host policy and a 30-second timeout,
and its parsed metadata must equal the previously authenticated document.
The plugin's secondary response has no byte-limit API, leaving a memory-use
risk if that trusted release endpoint serves an unexpectedly large response.
Immediately before install, native code re-verifies artifact size/hash/signature;
`Update::install` alone does not perform signature verification.
