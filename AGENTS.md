# ShaCraft Launcher — agent context

## Purpose and scope

Cross-platform desktop launcher for the ShaCraft Minecraft network. It is a
Tauri 2 application: React/Vite is the UI and Rust owns all filesystem,
network and process-adjacent work. The current production profile is
**Aeronautics** (Minecraft 1.21.1, NeoForge 21.1.248, Java 21).

This repository owns the launcher only. The server-side API and published
payload are in `/root/shacraft` on the ShaCraft host; see
`docs/launcher-architecture.md` before changing an integration boundary.

## Non-negotiable boundaries

- Launcher-managed payload is limited to ShaCraft-owned configuration and
  approved modpack files. Never make arbitrary URLs, shell commands, or local
  paths controllable by a remote manifest.
- Do not add private keys, OAuth secrets, `.env` values, player data, or built
  artifacts to Git.

## Trust model

- The only supported remote profile manifest endpoint is
  `https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest`.
  The read-only Aeronautics player-count endpoint
  `https://shacraft.ru/api/online/aoc` is also hardcoded in `remote.rs`; it
  is display-only and is never allowed to influence downloads or launching.
- The response is an Ed25519 envelope. `src-tauri/src/remote.rs` verifies its
  embedded public key and `keyId` **before** parsing the payload.
- `src-tauri/src/manifest.rs` then validates paths, SHA-256, sizes, HTTPS and
  allowed ShaCraft hosts. Do not weaken this whitelist.
- `src-tauri/src/profile.rs` downloads to a temporary sibling file, verifies
  size + SHA-256, and atomically replaces only launcher-managed files.
- This ShaCraft manifest is the **only** source of truth for which
  Minecraft version / NeoForge version / Java major a profile needs
  (`Manifest.minecraft`) and for mod/config files. It never supplies a URL
  for the game itself — see `docs/game-trust-boundary.md` for the four
  independent, hardcoded-host trust domains (Mojang, NeoForge, Microsoft,
  Adoptium) that install and run the actual game. Do not let manifest data
  control a URL in any of those domains.
- ShaCraft accounts: `src-tauri/src/shacraft_account.rs` talks only to the
  hardcoded `https://shacraft.ru` origin. Passwords are never persisted. The
  revocable session token is stored locally with mode 600 on Unix and never
  passed to Java. The new admission flow uses fixed POST endpoints
  `/api/launcher/v2/admission/nickname` and
  `/api/launcher/v2/admission/tickets`; redirects are rejected. Neither the
  manifest nor the webview can select their origin or URL.
- Free nicknames are claimed directly for the authenticated account. Existing
  player names are reserved server-side and assigned by an administrator;
  deleting a website account must not make an existing player's name free.
- After installation, `admission.rs` generates an ephemeral Ed25519 key with
  the OS CSPRNG. The backend binds its public key to a one-use ticket, current
  account session, canonical `aoc` nickname and server access. Only that
  returned nickname selects launch identity; `settings.json` is never a fallback.
  Ticket and PKCS#8 private key go only in the final Java child's environment
  (`SHACRAFT_ADMISSION_TICKET`, `SHACRAFT_ADMISSION_PRIVATE_KEY`). Never put
  them in global environment, argv/argfiles, settings, logs or IPC. A fresh
  launch obtains a fresh ticket; there is no shared launcher secret.
- Once admission is enforced on Aeronautics, its server mod verifies the
  challenge/proof before world entry, and server-side whitelist enforcement
  remains a final access boundary. Replace LoginSystem only as part of the
  validated `aoc` rollout; other servers are unaffected. Old launchers without
  admission proof will be rejected after enforcement. This authenticates an
  account's permission, not the integrity of an unmodified launcher binary.

## Layout

- `src/main.tsx` — React entrypoint; `src/App.tsx` composes the screen.
- `src/components/` — presentational UI; `src/hooks/` — lifecycle/settings/account.
- `src/services/native.ts` — typed IPC and event subscriptions; keep schemas
  aligned with Rust. `src/services/async.ts` — serialized writes, single-flight
  account restore and listener disposal. `src/state/` — tested reducers.
- Native filesystem/network/process operations never belong in the web layer.
- `src-tauri/src/` — native commands and security-sensitive logic.
  - `lib.rs` — module/command registration only; `commands/` holds adapters
    for account/game/host/preferences/profiles. Unsigned sync/inspect IPC was
    removed; only verified remote manifests may drive profile mutations.
  - `operations.rs` — process-local install/account permits owned by workers.
    The account permit covers ticket issuance through Java spawn, preventing
    local logout/account switching from racing that handoff.
  - `storage.rs` — unique same-directory atomic writes, owner-only Unix files.
  - `trusted_http.rs` — HTTPS and exact-host redirect policy per game provider.
  - `download.rs` — shared verified-download helper (temp file, hash,
    atomic rename, progress callback); `manifest.rs`/`profile.rs` (ShaCraft
    mods) and `mojang.rs`/`neoforge.rs`/`runtime.rs` (the game itself) all
    build on this rather than each rolling their own.
  - `mojang.rs` — vanilla Minecraft trust boundary + the generic
    `inheritsFrom` version-JSON merge (shared with NeoForge's profile).
  - `neoforge.rs` — runs NeoForge's official installer headlessly.
  - `runtime.rs` — Java 21 auto-provisioning via Eclipse Adoptium.
  - `msa.rs` — Microsoft/Xbox/Minecraft Services login; see
    `MSA_CLIENT_ID`'s doc comment before touching login — it is currently a
    placeholder pending ShaCraft's own Azure AD app registration and
    Minecraft-API approval.
  - `shacraft_account.rs` — local ShaCraft login/registration, session and
    nickname claim/admission API; legacy link commands remain for compatibility.
  - `admission.rs` — ephemeral key generation, strict ticket response validation,
    canonical identity and child-only admission environment.
  - `launch.rs` — builds and spawns the actual `java` process; admission secrets
    must remain outside its argument substitution and JVM argfile paths.
- `src-tauri/src/settings.rs` — durable local preferences; maintain backward
  compatibility with already-written JSON.
- `docs/manifest-v1.md` — signed manifest envelope and payload contract
  (mods/config only).
- `docs/game-trust-boundary.md` — the Mojang/NeoForge/Microsoft/Adoptium
  trust domains used to install and run the game itself.
- `.github/workflows/check.yml` — push/PR UI checks and Linux Rust tests.
- `.github/workflows/build.yml` — main-push/manual cross-platform builds with artifacts;
  not a signed release or updater publication.

## Verification

Run from repository root:

```bash
npm ci
npm test
npm run build
(cd src-tauri && /home/emil/.cargo/bin/cargo test)
npm run tauri:dev
```

`tauri:dev` is for local desktop testing. A successful web build alone does
not prove Tauri commands work.

See `PLAN.md` for known gaps. Never label browser preview or unit tests as
a successful cold game install / Microsoft OAuth / Windows/macOS beta test.
The admission implementation has local unit/UI checks and unsigned Linux
0.1.2 AppImage/deb packages built on Ubuntu 26.04. Older Ubuntu compatibility
has not been tested. The backend/mod rollout is active on Aeronautics as of
2026-09-10. Real isolated NeoForge admission tests and a production rejection
without the mod passed; these do not certify a full production modpack join.
The server has a required early duplicate-login guard so unauthenticated
connections cannot evict an already-online UUID. The client mod pins the actual
socket to `135.106.154.86:25567`. See architecture and server rollout records.

## Working conventions

- Keep UI copy in Russian; code, identifiers and errors may remain English.
- Prefer a small vertical slice with tests over unused abstractions.
- Existing working-tree changes belong to the user unless this task created
them. Inspect `git status` before staging.
- Update this file and `docs/launcher-architecture.md` whenever the trust
model, endpoint contract, storage layout, or release workflow changes.
