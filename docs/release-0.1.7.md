# Launcher 0.1.7 release — server migration

Published on 2026-09-17. Minigames Quick Play uses `shacraft.ru:25568`;
the Fabric admission companion accepts the new server's actual socket IP
`135.106.219.182`, the domain and the temporary old-IP forwarding path.

The owner approved replacing the unavailable updater signing key and requiring
one manual installation. Users of 0.1.6 and earlier must close Minecraft and
the launcher, then install 0.1.7 from [the download page](https://shacraft.ru/help#launcher).
Their installed game profiles do not need to be reinstalled. The new native
updater endpoint is `https://shacraft.ru/launcher/updates/stable-v2.json`.
The old `stable.json` remains byte-identical at 0.1.6 under its original key.
Never publish new-key signatures to that old channel.

## Provenance and verification

Runtime source: `b674347e867af3edb6f00201bd30f1db7af4d9d2` on
`codex/server-migration-20260917`.
All four jobs succeeded in [build run 35167814993](https://github.com/emil28092005/shacraft-launcher/actions/runs/35167814993).
[Checks run 35167815151](https://github.com/emil28092005/shacraft-launcher/actions/runs/35167815151)
passed native checks, publisher signature tests and the Java 25 Fabric build.
Downloaded artifact ZIP hashes match GitHub's SHA-256 digests and their run
metadata points to the exact runtime commit. Local checks passed 33 UI tests,
87 Rust tests (8 live/desktop tests ignored), TypeScript and Vite.

All eight packages were signed on the operator workstation, checked again on
the new server, and published after the metadata publisher's dry-run. Complete
public HTTPS downloads match the recorded hashes; all eight package signatures
and the v2 feed's metadata signature verify. The public feed bytes exactly match
the locally verified metadata and carry `Cache-Control: no-store`. The website
shows 0.1.7 links and manual installation instructions. See the
[publication receipt](verification/launcher-release-0.1.7.json).

Native executables extracted from Windows NSIS, Linux DEB and both macOS app
archives contain the domain endpoint, v2 channel and expected public key, with
no old IP literal. The CI-built Fabric companion was published at immutable
SHA-256 `6ef059192cef0242839db3d6a234228b771196f23b9b1e307a17a27669632a90`.
Its public profile signature uses the original profile key and verifies;
Java 25 target checks accept only the intended destinations. Minecraft status
queries work through both new and old public IPs (Paper 26.2, protocol 776).
This release verification does not establish a fresh Windows/macOS installation
or an authenticated game session on the migrated host. Authenticode and Apple
notarization remain separate from the updater signature.

## Operator state

The new updater private key is only at
`/home/emil/.local/share/shacraft-updater/production.key` (0600; parent 0700).
Its public companion's SHA-256, after trimming whitespace, is
`5ac34dd380307ab25fcfc1479ee6d653b9b43bce45b5a242950d09c01fe1b2f9`.
Maintain an encrypted owner-controlled backup. No private key was uploaded to
GitHub or the server. Future versions must use this same key and the v2 feed.

Production is `135.106.219.182`. Release evidence is under
`/root/shacraft/.release-staging/launcher-0.1.7`, with the exact runtime source
snapshot at `/root/shacraft-launcher-0.1.7`. The unversioned launcher checkout on
that host is older. The website image is `shacraft/backend:migration-017-20260917`.

The old host `135.106.154.86` only forwards ports 80, 443 and 25568. Keep it
until authoritative DNS and caches have updated **and** users of 0.1.6 have
manually upgraded: those binaries pin the old IP independently of DNS.
Do not restart old application containers or Hermes. Detailed migration and
rollback context is in `/context/migration-20260917.md` on the new host.
