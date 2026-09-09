# Game trust boundary (Mojang / NeoForge / Microsoft / Adoptium)

`docs/manifest-v1.md` and `AGENTS.md` cover the ShaCraft-signed manifest,
which governs **only** mods, configs, and which Minecraft/NeoForge/Java
version a profile needs. This document covers the four separate,
independently-hardcoded trust domains that install and run the game itself.
None of their hosts or URLs are ever taken from the ShaCraft manifest, and
the manifest can never point the launcher at a different host for any of
them — that boundary is load-bearing, not incidental.

## 1. Mojang (`mojang.rs`)

Hosts: `piston-meta.mojang.com`, `piston-data.mojang.com`,
`libraries.minecraft.net`, `resources.download.minecraft.net`.

Every artifact's SHA-1 comes from a parent document that was itself SHA-1
verified, back to `version_manifest_v2.json`: manifest -> version JSON ->
client jar / each library / asset index -> each asset object. `mojang.rs`
also owns the generic `inheritsFrom` version-JSON merge — the same
loader-agnostic algorithm any vanilla-compatible launcher uses to run a
Forge/NeoForge/Fabric profile, because that format is specifically designed
so third-party launchers don't need per-loader special-casing.

## 2. NeoForge (`neoforge.rs`)

Host: `maven.neoforged.net` only.

Downloads the official installer jar for `manifest.minecraft.loader.version`
and verifies it against the `.sha256` sidecar Maven publishes next to every
artifact (confirmed live, byte-for-byte). Runs it headlessly:
`java -jar neoforge-<ver>-installer.jar --installClient <game_dir>` — its
real main class is `net.minecraftforge.installer.SimpleInstaller`, which
supports this flag. **Empirically verified (2026-09-06)**: it refuses to
target a directory unless a `launcher_profiles.json` stub already exists
there ("you need to run the launcher first!") — `ensure_launcher_profiles_stub`
writes a minimal one. It fetches the inputs needed to patch vanilla, but does
not guarantee that the complete vanilla runtime library set is present.
After installation, `mojang::ensure_client_jar` and `ensure_libraries` always
verify and download the complete merged launch set, including LWJGL and its
platform natives. The installer's own downloads go straight to
`maven.neoforged.net`/Mojang, outside our control — an accepted trust
delegation to NeoForge's official tooling once the installer binary itself is
verified.

Also verified: the resulting
`libraries/net/neoforged/neoforge/<ver>/neoforge-<ver>-client.jar` (the
patched, deobfuscated client) is **not** part of the generic classpath and
must not be added to it — FancyModLoader locates and loads it itself at
runtime via the `--fml.*` game arguments already present in the merged
profile. The classpath is just the ordinary union of rule-allowed vanilla +
NeoForge libraries plus the *vanilla* client jar (`client_jar_version_id` on
`MergedVersion`, not the NeoForge profile's own id — it has no jar of its
own on disk, confirmed).

## 3. Microsoft / Xbox Live / Minecraft Services (`msa.rs`)

Hosts: `login.microsoftonline.com`, `user.auth.xboxlive.com`,
`xsts.auth.xboxlive.com`, `api.minecraftservices.com`.

This module is retained for a future Microsoft mode; the current launcher
uses authenticated ShaCraft account links and deterministic offline identity.
Do not treat this unused module as the active launch gate.

In a Microsoft flow: device-code OAuth -> Xbox Live user token -> XSTS token ->
Minecraft Services login -> `GET /minecraft/profile` ownership check. An
authentication/ownership failure must never fall back to another identity. See
`MSA_CLIENT_ID`'s doc comment in `msa.rs`: unlike the other three domains,
this one needs a deployment-specific value — ShaCraft's own Azure AD app
registration, approved for Minecraft API access via
`https://aka.ms/mce-reviewappid`. `start_device_code`/`refresh_microsoft_tokens`
refuse to run while it's still the placeholder.

## 4. Eclipse Adoptium (`runtime.rs`)

Host: `api.adoptium.net` (redirects to `github.com`/
`objects.githubusercontent.com`/`release-assets.githubusercontent.com` for the download — expected, still
verified).

Java 21 JRE, GPLv2+CE. The API returns the release's SHA-256 inline, verified
before extraction. Never touches a Java installation the user already has —
`java::ensure_java` only provisions here when `java::detect()` finds nothing
with exactly the manifest's `javaMajor`; a newer major is not assumed
compatible with the Minecraft/NeoForge version.

## Why this separation matters

Each domain is hardcoded and verified independently so that a compromised or
malicious ShaCraft manifest — or a bug that lets manifest data flow into a
URL — cannot redirect a download to an attacker-controlled host in any of
these domains. When adding a new game-related download, verify its host is
one of the ones above (or add a new hardcoded constant following the same
pattern) rather than accepting a URL from anywhere else.
