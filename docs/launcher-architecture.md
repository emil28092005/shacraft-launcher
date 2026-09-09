# ShaCraft Launcher architecture

## Current capability

The launcher persists local settings, synchronises Aeronautics mod/config
files from the signed ShaCraft v2 manifest, installs the exact Minecraft +
NeoForge version the manifest specifies, and launches the game. A player
signs in with the same local ShaCraft account used on the website. The game
identity is derived only from that account's verified Aeronautics nickname;
the legacy editable nickname setting is not trusted at launch.

The interface also shows a live Aeronautics player count from the fixed,
read-only `https://shacraft.ru/api/online/aoc` endpoint. It is display-only:
the result never controls files, versions, URLs, or the launch command.

Not yet implemented: a user-selectable profile directory, a "reset managed
files only" recovery action, and signed cross-platform release builds of the
launcher itself. Do not represent these as completed in UI or release notes.

## Data flow

Two independent pipelines feed one launch:

```text
ShaCraft manifest (mods/config + which MC/loader/Java version to use)
  signed-manifest endpoint -> Ed25519 verification (remote.rs)
  -> manifest schema + URL/path validation (manifest.rs)
  -> temporary download, SHA-256 verification, atomic replacement (profile.rs)

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
3. Signed, cross-platform release builds of the launcher itself.
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
These are process-local guards, not cross-process locks or cancellation.
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
The package workflow runs on main pushes or manually and builds Windows
x64, Linux x64 and both macOS architectures with named artifacts.
Packages are not yet signed release artifacts. Native cold-install and
launch tests are required before calling a platform release-ready.
