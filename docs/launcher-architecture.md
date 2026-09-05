# ShaCraft Launcher architecture

## Current capability

The launcher can persist local settings, discover an installed Java runtime,
inspect a profile and synchronise Aeronautics files from the signed ShaCraft
v2 manifest. It does **not** launch Minecraft yet.

## Data flow

```text
signed-manifest endpoint
  -> Ed25519 verification in remote.rs
  -> manifest schema + URL/path validation
  -> local profile inspection
  -> temporary download, SHA-256 verification, atomic replacement
```

Profiles live below Tauri's `app_data_dir()/profiles/<profile-id>`. Settings
live at `app_data_dir()/settings.json`. Neither location should be assumed to
be the system `.minecraft` directory.

## Aeronautics contract

- Profile ID: `aeronautics`
- Manifest endpoint:
  `https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest`
- Payload: manifest schema v1, 251 managed files at the time of writing.
- Download files: HTTPS only, exact hosts `shacraft.ru` and
  `cdn.shacraft.ru`.

## Planned but not implemented

1. Signed, cross-platform Java 21 runtime installation.
2. User-selectable profile directory and structured launcher logs.
3. Official Microsoft authentication and a compliant Minecraft/NeoForge launch
   flow.

Do not represent these as completed features in UI or release notes.
