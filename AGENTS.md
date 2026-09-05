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

- Do not implement licence bypasses, fake Minecraft access tokens, or download
  or redistribute Minecraft game assets. Keep game authentication/launching
  separate from modpack management.
- Launcher-managed payload is limited to ShaCraft-owned configuration and
  approved modpack files. Never make arbitrary URLs, shell commands, or local
  paths controllable by a remote manifest.
- Do not add private keys, OAuth secrets, `.env` values, player data, or built
  artifacts to Git.

## Trust model

- The only supported remote profile endpoint is
  `https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest`.
- The response is an Ed25519 envelope. `src-tauri/src/remote.rs` verifies its
  embedded public key and `keyId` **before** parsing the payload.
- `src-tauri/src/manifest.rs` then validates paths, SHA-256, sizes, HTTPS and
  allowed ShaCraft hosts. Do not weaken this whitelist.
- `src-tauri/src/profile.rs` downloads to a temporary sibling file, verifies
  size + SHA-256, and atomically replaces only launcher-managed files.

## Layout

- `src/main.tsx` — UI state and Tauri command calls; do not put privileged
  operations in the web layer.
- `src-tauri/src/` — native commands and security-sensitive logic.
- `src-tauri/src/settings.rs` — durable local preferences; maintain backward
  compatibility with already-written JSON.
- `docs/manifest-v1.md` — signed manifest envelope and payload contract.
- `.github/workflows/build.yml` — manual cross-platform build matrix.

## Verification

Run from repository root:

```bash
npm run build
(cd src-tauri && /home/emil/.cargo/bin/cargo test)
npm run tauri:dev
```

`tauri:dev` is for local desktop testing. A successful web build alone does
not prove Tauri commands work.

## Working conventions

- Keep UI copy in Russian; code, identifiers and errors may remain English.
- Prefer a small vertical slice with tests over unused abstractions.
- Existing working-tree changes belong to the user unless this task created
them. Inspect `git status` before staging.
- Update this file and `docs/launcher-architecture.md` whenever the trust
model, endpoint contract, storage layout, or release workflow changes.
