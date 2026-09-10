# ShaCraft Launcher architecture

## Current capability

The launcher persists local settings, synchronises Aeronautics mod/config
files from the signed ShaCraft v2 manifest, installs the exact Minecraft +
NeoForge version the manifest specifies, and launches the game. A player
signs in with the same local ShaCraft account used on the website. The current
admission implementation claims a free nickname for that account and requests
a one-use permission immediately before launching Java. The game identity
comes only from the canonical Aeronautics nickname in that permission; the
legacy editable nickname setting is not trusted at launch.

The backend and admission mod were deployed to Aeronautics on 2026-09-10;
the server is healthy with whitelist enforcement retained. Real isolated
NeoForge connections verified successful admission, absent/replayed proof
rejection and protection of an online player from duplicate login. A public
production connection without the mod was rejected before world entry.
This does not certify a full cold installation or Windows/macOS operation.

The interface also shows a live Aeronautics player count from the fixed,
read-only `https://shacraft.ru/api/online/aoc` endpoint. It is display-only:
the result never controls files, versions, URLs, or the launch command.

Version 0.1.3 adds signed application updates, separate from modpack sync.
Version 0.1.4 adds installed deb updates with system administrator confirmation.
The first AppImage upgrade from 0.1.2 and deb upgrade from 0.1.3 are manual.
Windows/macOS 0.1.5 packages are published; actual desktop installation tests remain pending.
Not yet implemented: a user-selectable profile directory and a "reset managed
files only" recovery action. OS code signing/notarization is separate from the
updater signatures and is not certified by this implementation.

## Data flow

Managed files, game installation and account admission meet at Java spawn:

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

Admission (fixed ShaCraft account API; never controlled by a manifest)
  native website session -> direct free-nickname claim or admin migration
  -> after installation: OS CSPRNG -> ephemeral Ed25519 key (admission.rs)
  -> public key + bearer session -> one-use ticket for canonical aoc nickname
  -> deterministic offline UUID for that nickname (session.rs)
  -> Java with merged classpath/args + child-only ticket/private-key environment
  -> client mod signs server challenge; server validates before world entry
```

Profiles (ShaCraft-managed mods/config, and the player's own worlds/
screenshots/resourcepacks) live below Tauri's `app_data_dir()/profiles/
<profile-id>` — this becomes `--gameDir`. The shared vanilla+NeoForge
install (versions/libraries/assets/runtime, reused across profiles that
target the same Minecraft version) lives at `app_data_dir()/game`. Settings
live at `app_data_dir()/settings.json`, and the revocable ShaCraft session at
`app_data_dir()/shacraft-session` (mode 600 on Unix). Passwords are never
written to disk. The admission private key and ticket are ephemeral native
values and are not persisted. None of these directories should be assumed to
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
- Launch identity: the canonical `aoc` nickname returned with the admission
  ticket. Local nickname edits cannot select an identity.

## Admission contract (implementation: 2026-09-10)

Both endpoints use the fixed account API origin, HTTPS without redirects and
the native website session as bearer authentication. The session is never
passed to the client mod or Java process.

`POST /api/launcher/v2/admission/nickname` accepts
`{server_id: "aoc", mc_username: "Chosen_Name"}` and returns the existing
account shape `{username, links}`. The backend atomically assigns a free name
to that account. Existing player names remain reserved and require explicit
administrator migration. The launcher no longer asks the player to enter the
game to complete this claim. Legacy challenge IPC remains available to older
flows, but an admission-enabled backend must block those legacy endpoints
from bypassing reserved-name ownership.

After installation completes, the native launcher generates a fresh Ed25519
key using the OS CSPRNG and calls
`POST /api/launcher/v2/admission/tickets` with
`{server_id: "aoc", public_key: "<standard base64 raw 32-byte public key>"}`.
The response is `{ticket_id, mc_username, server_id, expires_in_seconds}`.
The native boundary requires a canonical 43-character base64url ticket ID,
an ASCII Minecraft nickname of 3–16 letters/digits/underscores, server `aoc`
and a positive lifetime of at most 600 seconds. The backend checks the current
account session, bound nickname and server access before issuing it.

`admission.rs` has no secret-bearing `Debug` or `Serialize` implementation.
Only the final Java child's environment receives:

- `SHACRAFT_ADMISSION_TICKET`: the one-use ticket ID.
- `SHACRAFT_ADMISSION_PRIVATE_KEY`: standard base64 of the Ed25519 PKCS#8
  private-key-only DER representation accepted by Java's `KeyFactory`.

No global environment mutation, webview/IPC payload, argument substitution,
JVM argfile, settings file or log stores these values. The account operation
permit remains held from issuance through spawn so local account switching
or logout cannot race the handoff. Missing, disabled or invalid admission
responses fail the launch with a visible error; there is no legacy-name or
unsigned fallback. In this first version, a consumed or expired ticket needs
a fresh game launch from the launcher; transparent in-game reconnect is not
implemented.

The client mod proves possession of the ephemeral private key by signing the
server's challenge. The server mod gates world entry on successful backend
verification, including one-use consumption and current account/link/access
checks. Whitelist enforcement is retained. The 2026-09-10 Aeronautics rollout
replaced its separate LoginSystem `/register` and `/login` flow; other servers
keep their existing authentication. Old launchers without admission proof
cannot join Aeronautics. A required server-only Mixin rejects a duplicate
online UUID before vanilla can disconnect the existing player. The client pins
the actual game socket to `135.106.154.86:25567`; changing that address requires
an explicit mod update. Server source and rollout records live in
`/root/shacraft/services/admission-mod` and `docs/admission-2026-09-10.md` on
the ShaCraft host.

This protocol prevents entry without the account's current permission. It
does not attest that an original launcher or game binary is unmodified:
software running as the same user can read its own process environment, and
a compatible client can implement the protocol. Never replace account-bound
proof with a shared key embedded in distributed binaries.

## Planned but not implemented

1. User-selectable profile directory and structured launcher logs.
2. "Reset managed files only" recovery action that doesn't touch player
   worlds/screenshots/resourcepacks.
3. Windows Authenticode/macOS signing-notarization and cross-platform release
   installation testing.
4. Cancellation, structured logs and a full cold-install/recovery beta on
   every target OS. Install progress reports bytes or installer work counts
   depending on the stage; these units are not interchangeable.
5. Transparent reconnect after the admission ticket has been consumed or
   expired. The current implementation requires a new launch.

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

## Signed application updates (0.1.3)

`updater.rs` accepts only the fixed HTTPS feed
`https://shacraft.ru/launcher/updates/stable.json`. A dedicated embedded Tauri
public key authenticates both the metadata payload and the selected package.
The signed metadata binds the plain stable version, release notes, date and
platform URLs. Artifact URLs are confined to the matching version directory
under `https://shacraft.ru/downloads/shacraft-launcher/`. The IPC never accepts
a URL, key, destination path or replacement executable from the webview.

Metadata is downloaded once, with a 192 KiB envelope/64 KiB payload bound;
packages are capped at 256 MiB. Redirects and version downgrades are rejected.
The small vendored Tauri 2.11.0 `check_metadata` patch constructs its update
object without another HTTP request. Linux AppImage installation uses a
same-directory temporary file, signature verification, preserved permissions,
atomic rename and file/directory fsync. Windows/macOS retain Tauri's platform
installers. Unsupported Linux formats show manual installation instructions.

For installed deb packages, 0.1.4 selects only `linux-x86_64-deb`. The feed also
retains the identical legacy `linux-x86_64` and explicit `linux-x86_64-appimage`
AppImage entries so installed 0.1.3 readers remain compatible. Remote metadata
cannot switch a deb installation into an AppImage installation.

`deb_updater.rs` checks root ownership of the installed executable and its
parents and dpkg's ownership/version record. One `pkexec` invocation starts an
early non-GUI mode of `/usr/bin/shacraft-launcher`; no password is collected by
the launcher and no fallback prompt runs after cancellation. Bounded stdin
framing carries the signed envelope and package bytes, never user paths.
The helper authenticates both again as root, checks exact package identity
`sha-craft-launcher`, architecture and version, stages the package under a
root-only temporary directory and invokes the fixed dpkg installer. An explicit
`--refuse-downgrade` protects against another installation winning the version
race. Failed/partial package transactions require honest system-package recovery;
they are not reported as completed or automatically retried. Restart launches
the fixed installed executable even after dpkg replaces the running inode.

The native updater holds installation, account and game permits while installing
and until restart. The game permit remains held until the launched Java child
exits. These guards cover this launcher process, not other launcher instances.
Startup checks never silently install; settings expose check, install, progress,
errors and restart. A failed check does not prevent using the installed version.

The private updater key stays on the operator's computer. Normal CI builds are
explicitly unsigned; reviewed release artifacts and metadata are signed locally
and published only after signature/hash verification. The Caddy feed route uses
`Cache-Control: no-store`. See [launcher-updates.md](launcher-updates.md) for
the envelope contract, publisher commands and recovery constraints.

## Verification and distribution

`npm test` covers asynchronous helpers and state transitions;
`npm run build` runs strict TypeScript before Vite. `cargo test --locked`
covers native policy and storage. Push/PR CI repeats checks on Linux.
The package workflow runs on main pushes or manually and builds Windows
x64, Linux x64 and both macOS architectures with named artifacts.
CI packages are unsigned build artifacts. Signed updater publication is a
separate local operator step. Native cold-install and launch tests are required
before calling a platform release-ready.

The local admission checkpoint passed 64 Rust tests (5 live tests ignored),
22 UI unit tests, TypeScript/Vite build and a Linux x86-64 release build with
`npm run tauri:build -- --no-bundle`. Six browser scenarios with mocked Tauri
IPC covered invalid nicknames, deleted sessions, reserved-name errors,
successful claims, duplicate clicks and a session revoked during a claim.
They also checked modal feedback, Escape preserving the settings drawer and
the absence of legacy link polling. These checks used no production account
or real Minecraft connection. The Linux output is a dynamically linked
binary, not evidence of Windows/macOS support testing.

Version 0.1.2 also produced unsigned Linux amd64 AppImage and deb packages with
`npm run tauri:build -- --bundles appimage,deb`. AppImage extraction and the
deb's version/architecture metadata were checked without running the app or
installing the package. The build host was Ubuntu 26.04; do not claim support
for older Ubuntu releases from this build. For local AppImage packaging,
linuxdeploy's GTK plugin needs `librsvg-2.0.pc` from the matching `librsvg2-dev`
package. Extracting that package into a temporary build directory and setting
`PKG_CONFIG_PATH` supplied the missing metadata without changing host packages.

The 0.1.3 updater checkpoint passed 76 native tests (6 live tests ignored),
28 UI tests, 11 publisher tests with real minisign and TypeScript/Vite build.
Nine browser scenarios used mocked IPC. The signed Linux AppImage/deb were
published on 2026-09-10 with a signed stable feed; feed bytes, signatures and
public HTTPS responses were verified. A separately invoked live native test
downloaded the production release, rejected corrupted bytes without changing
the old file, atomically updated a temporary copy of 0.1.2 and compared hashes.
The original source AppImage was retained. The installed 0.1.3 AppImage was
then started from `~/Applications` and its captured runtime paths verified.
This is not a full GUI update/restart cycle or a Windows/macOS installation test.
The Linux build host remains Ubuntu 26.04.

The 0.1.4 checkpoint passed 84 native tests (6 ignored), 32 UI tests and 18
publisher tests; 12 browser scenarios use mocked IPC. In a disposable Ubuntu
26.04 Docker container without network or production mounts, the actual signed
deb helper passed 10 scenarios: unprivileged invocation, truncated/trailing
input, damaged metadata/package, dpkg lock, unsafe temporary directory,
successful installation, replay and downgrade refusal. The fixture installed
the genuine old 0.1.3 package and bootstrapped the new verifier binary over its
package record; it then installed the genuine signed 0.1.4. It did not relabel
signed versions. Dpkg reported 0.1.4 and a fixture profile marker survived.
This tests the elevated helper and dpkg, not a real desktop PolicyKit dialog.
Cancellation/error rendering is covered by unit/browser scenarios. Published
metadata and deb bytes match the locally verified files; both website download
buttons target 0.1.4. No user host package installation was performed for QA.
The live native AppImage smoke also passed against the published 0.1.4 feed,
including corruption rejection and replacement of only a temporary source copy.


## Cross-platform release 0.1.5 (2026-09-10)

Published Windows x64 EXE/MSI, macOS aarch64 and x86_64 DMG/app.tar.gz,
and Linux x64 AppImage/DEB on https://shacraft.ru/help#launcher. The signed
stable feed includes all platforms plus the legacy/exact Linux aliases.
CI source b43fc610c43a9ec9f5f3ffce601de9604670ff8a, successful Actions run
https://github.com/emil28092005/shacraft-launcher/actions/runs/34512683651.
An initial non-Linux borrow/move compilation error in updater target selection
was fixed before the final build. Native tests: Windows70, macOS74 per arch,
Linux84; UI tests pass on all four runners. Local checks verify macOS bundle
version/CPU type, Linux package identity and every artifact signature. Public
HTTPS downloads of all eight artifacts match local SHA-256. Website418 tests
plus48 subtests pass; only backend recreated, game containers unchanged.
Updater signatures use the existing operator-held key, never uploaded to CI or
server. Windows installers have no Authenticode signature; macOS is not Apple
notarized. Native CI tests and packaging do not certify full Minecraft installs
or desktop updater/restart behavior on Windows/macOS. Older unsupported clients
need a manual installation of the current release. Previous releases immutable.

The live native updater test passed against the published 0.1.5 feed: verified download, corrupted-byte rejection and replacement of only a temporary AppImage copy. The user-installed launcher was not modified.
