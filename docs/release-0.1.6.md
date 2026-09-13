# Launcher 0.1.6 release

Adds a separate Minigames profile (Minecraft 26.2 / Fabric 0.19.5 / Java 25)
while keeping Aeronautics and its installed profile intact. Both use the
existing canonical aoc nickname and shared access. Tickets remain bound to
the selected server. Minigames connects to 135.106.154.86:25568 and proves
account admission during configuration before world entry.

Local checks: 87 native tests, 33 UI tests, TypeScript/Vite, three Java client
proof tests, an official Fabric metadata resolution check and the Linux native
release build. The [real Fabric/Paper smoke](verification/minigames-fabric-2026-09-13.json)
passed with a synthetic account: the unchanged production companion entered
the lobby and the backend consumed its Minigames ticket. Published on 2026-09-13:
all four cross-platform jobs succeeded in [build run 34778218510](https://github.com/emil28092005/shacraft-launcher/actions/runs/34778218510)
at runtime source `799fa692ef5e775f0044fe300f65fd4564771d47`.
[Checks run 34778334417](https://github.com/emil28092005/shacraft-launcher/actions/runs/34778334417)
passed at `5b741771b4668a834a2c5b757379340f11b8ae3a`; the only difference is the
CI job that runs all 18 publisher signature tests on Ubuntu 24.04. The CI Fabric
jar exactly matches the one used by the real client smoke.

All eight packages were signed locally, verified again on the server, and
published after a successful dry-run. Full public HTTPS downloads match the
recorded SHA-256 values; package signatures and the public feed signature pass.
The feed payload equals the locally signed canonical bytes. See the
[publication receipt](verification/launcher-release-0.1.6.json). The previous
0.1.5 feed was backed up; its packages remain unchanged. The private key stayed
on the operator workstation.

## Build contract

Git remote: `git@github.com:emil28092005/shacraft-launcher.git`.
The production admission line is branch `codex/launcher-updater`; `main` is a
divergent unreleased prototype and must remain unchanged for this release.
Push the reviewed commit to the production branch, then dispatch
`gh workflow run build.yml --ref codex/launcher-updater --repo emil28092005/shacraft-launcher`.
The check workflow runs on branch pushes. Verify each run's head SHA equals the
reviewed commit before downloading artifacts. A main push also triggers the
build matrix, but was not the publication path for this release.
CI publishes unsigned artifacts named:

- `shacraft-launcher-linux-x64`: AppImage and deb.
- `shacraft-launcher-windows-x64`: NSIS exe and MSI.
- `shacraft-launcher-macos-arm64`: aarch64 app.tar.gz and DMG.
- `shacraft-launcher-macos-x64`: x86_64 app.tar.gz and DMG.

`.github/workflows/check.yml` additionally tests the Java 25 admission client
and uploads `shacraft-admission-client`. It has no production credentials.
Download the artifacts from the checked run at the exact reviewed commit with
`gh run download RUN_ID --repo emil28092005/shacraft-launcher --dir STAGING`.

## Signing and publication

Stage renamed ASCII filenames below a local downloads root, for example
`/tmp/shacraft-release-0.1.6/downloads/0.1.6/`. Preserve already published
0.1.5 bytes. Expected updater filenames:

- `ShaCraft.Launcher_0.1.6_amd64.AppImage`
- `ShaCraft.Launcher_0.1.6_amd64.deb`
- `ShaCraft.Launcher_0.1.6_x64-setup.exe`
- `ShaCraft.Launcher_0.1.6_aarch64.app.tar.gz`
- `ShaCraft.Launcher_0.1.6_x86_64.app.tar.gz`

MSI and DMG are additional manual downloads; the updater uses EXE and app.tar.gz.
Inspect package versions, architecture and contents before signing. Sign each
chosen file with the existing local operator key:

```bash
npm run tauri -- signer sign --private-key-path /home/emil/.local/share/shacraft-updater/production.key ARTIFACT
```

The key file is mode 0600 and remains local. Never read its contents into logs,
copy it to CI/server or substitute a different signing identity. Existing
`production.key.pub` is sufficient for every later verification/publication.

After creating a UTF-8 release notes file, prepare the payload:

```bash
python3 scripts/publish_launcher_update.py prepare \
  --version 0.1.6 \
  --downloads-root /tmp/shacraft-release-0.1.6/downloads \
  --artifact linux-x86_64=ShaCraft.Launcher_0.1.6_amd64.AppImage \
  --artifact linux-x86_64-appimage=ShaCraft.Launcher_0.1.6_amd64.AppImage \
  --artifact linux-x86_64-deb=ShaCraft.Launcher_0.1.6_amd64.deb \
  --artifact windows-x86_64=ShaCraft.Launcher_0.1.6_x64-setup.exe \
  --artifact darwin-aarch64=ShaCraft.Launcher_0.1.6_aarch64.app.tar.gz \
  --artifact darwin-x86_64=ShaCraft.Launcher_0.1.6_x86_64.app.tar.gz \
  --notes-file /tmp/shacraft-release-0.1.6/notes.txt \
  --public-key /home/emil/.local/share/shacraft-updater/production.key.pub \
  --payload /tmp/shacraft-release-0.1.6/release.payload.json
npm run tauri -- signer sign \
  --private-key-path /home/emil/.local/share/shacraft-updater/production.key \
  /tmp/shacraft-release-0.1.6/release.payload.json
```

Upload only public packages, signatures, payload, public key and publisher.
Server downloads root is `/root/shacraft/caddy/www/downloads/shacraft-launcher`;
public artifact URLs are `https://shacraft.ru/downloads/shacraft-launcher/0.1.6/`
followed by the checked filename. Preserve the old feed before publication.
With the exact staged server paths, run the existing publisher first with
`--dry-run`, then without it:

```bash
python3 publish_launcher_update.py publish \
  --downloads-root /root/shacraft/caddy/www/downloads/shacraft-launcher \
  --public-key PUBLIC_KEY_FILE \
  --payload SIGNED_PAYLOAD_FILE \
  --signature PAYLOAD_SIGNATURE_FILE \
  --output /root/shacraft/data/launcher/updates/stable.json \
  --dry-run
```

Publication depends on the signed Minigames profile containing the final client
and Fabric API jars, the healthy Paper admission gate, and the shared-access
backend endpoints being available. Verify public HTTPS package hashes, feed
signatures and an actual client admission before updating the website buttons.
OS Authenticode/Apple notarization and cold installations on other platforms
remain distinct from successful native CI/builds.
