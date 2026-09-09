# Launcher updater releases

The updater has its own signing key and fixed repository:
`emil28092005/shacraft-launcher`. It is independent of the ShaCraft modpack
manifest key. The committed public key is `src-tauri/updater-public-key.txt`;
the native client and release validation pin that key. An unset/placeholder key
cannot produce a release.

## Packages and signed metadata

The four build targets each produce two distributable files, with canonical names:

- Windows x64: `shacraft-launcher_VERSION_windows-x86_64-setup.exe` (NSIS)
  and `shacraft-launcher_VERSION_windows-x86_64.msi`.
- Linux x64: `shacraft-launcher_VERSION_linux-x86_64.AppImage` and
  `shacraft-launcher_VERSION_linux-x86_64.deb`.
- macOS arm64: `shacraft-launcher_VERSION_darwin-aarch64.dmg` and
  `shacraft-launcher_VERSION_darwin-aarch64.app.tar.gz`.
- macOS x64: `shacraft-launcher_VERSION_darwin-x86_64.dmg` and
  `shacraft-launcher_VERSION_darwin-x86_64.app.tar.gz`.

Every package has a detached `.sig` from the Tauri signer. AppImage, NSIS, MSI
and app tar signatures must already exist after `tauri build`; absence is an
error. The collection step signs deb and DMG explicitly. Renaming a package does
not alter its bytes or signature. The macOS app archive contains the `.app`
bundle; the DMG remains the manual installation package. These artifact formats
follow [Tauri's updater documentation](https://v2.tauri.app/plugin/updater/).

`latest.json` contains exactly `schemaVersion` (integer 1), `version`, `tag`,
`notes`, `pub_date`, `platforms` and `manualPackages`. `version` is stable
`MAJOR.MINOR.PATCH`; `tag` is exactly `vVERSION`. Windows MSI limits apply:
major/minor at most 255 and patch at most 65535. `pub_date` uses UTC
`YYYY-MM-DDTHH:MM:SSZ`, derived from the tagged source commit.

`platforms` has exactly `windows-x86_64`, `linux-x86_64`, `darwin-aarch64` and
`darwin-x86_64`. Their primary packages are NSIS, AppImage and the two app archives.
`manualPackages` has exactly `windows-x86_64-msi`, `linux-x86_64-deb`,
`darwin-aarch64-dmg` and `darwin-x86_64-dmg`. Despite the section name, the native
client also selects the MSI descriptor when updating an MSI installation.

Each descriptor has exactly `url`, `signature`, `sha256` and `size`. URLs are
bound to the exact version/tag and canonical filename under
`https://github.com/emil28092005/shacraft-launcher/releases/download/vVERSION/`.
SHA-256 is lowercase hexadecimal; size is a positive integer at most 1 GiB.
Metadata is at most 32 KiB, notes at most 4096 UTF-8 bytes and signatures at most
2048 characters.

The release script writes deterministic UTF-8 JSON with sorted keys, two-space
indentation and a trailing newline. It signs those exact bytes into
`latest.json.sig` using the same updater key. The client verifies this signature
before parsing: an artifact signature alone cannot bind a version or download
URL. The signed metadata binds the complete platform set, versions, filenames,
hashes, sizes and package signatures together.

`scripts/release-verifier` uses `minisign-verify` 0.2.5 and the same verification
sequence as the [Tauri updater 2.11 implementation](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs):
base64-decode the Tauri public key/signature containers, decode the minisign
objects, then verify the exact file bytes including the trusted comment. Missing,
nonempty-but-invalid and mismatched-key signatures all fail. The utility does
not execute installers or replace applications.

## CI and protected draft creation

`check.yml` runs the usual UI/Rust checks plus release tests. `build.yml` builds
all four targets with fresh disposable test keys. It has read-only repository
permissions and receives no production signing secret. Transient build config
and `SHACRAFT_UPDATER_TEST_BUILD=1` select the test key; no committed pin changes.
Artifacts are labelled `CI-NOT-FOR-RELEASE-*`, include `CI_NOT_FOR_RELEASE.txt`,
and cannot pass release validation. Test keys are never uploaded.

Before a release, an operator must configure two existing GitHub environments:
`launcher-release` and `launcher-release-publish`. Both require a reviewer other
than the dispatcher (`prevent_self_review=true`) and permit protected branches
only. `main` must be protected. This is a two-person operation: the dispatcher
cannot approve their own deployment. The workflow gate checks these settings
through GitHub's [environment API](https://docs.github.com/en/rest/deployments/environments)
before exposing an environment name to later jobs. Missing environments or
insufficient API permissions stop the workflow; it never creates an unprotected
replacement or bypasses the check.

Set `SHACRAFT_UPDATER_PUBLIC_KEY` as an environment/repository variable, equal to
the complete committed base64 public key. `launcher-release` alone needs the
`TAURI_SIGNING_PRIVATE_KEY` secret (official Tauri encoded key contents) and,
for an encrypted key, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The password may be
omitted/empty for an unencrypted key; signing is noninteractive. The publish
environment needs only the public variable, never the private key.

The private key must live outside Git, build artifacts and web roots. Keep the
key directory owner-only (0700), its key file owner-only (0600), and retain a
separate secure backup outside Git. Do not print the key, upload it as an
artifact or keep a purported encryption password beside it. Losing the key
breaks updates for installed clients; changing the public key is a coordinated
migration, not routine key regeneration.

To prepare a release:

1. Merge the reviewed source/version change. Package JSON, npm lock, Cargo
   package and Tauri config versions must match. Create the exact `vVERSION`
   tag on a commit belonging to protected `main`; the workflow never creates or
   moves a tag.
2. Manually run **Prepare signed release draft** from `main` with that version,
   tag and release notes. Pass the independent environment approval. Every
   matrix runner checks the tag/versions and signs a challenge to prove the
   supplied private key matches the committed public key before packaging.
3. All four jobs must finish. Only then does the final job assemble all eight
   packages, all eight signatures and signed metadata. It verifies this complete
   18-file set before creating a GitHub draft, uploads it, downloads it again
   and re-verifies the draft. A partial upload remains a draft and cannot pass
   publication validation. An existing draft is never silently overwritten.

Before any candidate source runs, trusted workflow Git commands prove that the
tag commit belongs to protected main. Later jobs check out the validated immutable
SHA, not the movable tag. Checkout credentials are removed, and the publication
token exists only in the final explicit publishing step.

The tag/release workflow is manual; pushing a tag does not publish an update.
The scripts do not configure GitHub environments, keys or secrets. Repository
access alone is not authorization to run the publication workflow.

## Explicit final publication

After reviewing the draft and completing platform acceptance, an operator runs
**Publish verified release (operator only)** from `main`, supplies version/tag
and the exact confirmation `publish vVERSION`, then obtains the separate
publish-environment approval. This job has no signing key. It checks the source
versions/tag, current committed public pin, environment policy and monotonic
stable release version; downloads every draft asset and checks all signatures,
hashes, sizes, names and signed metadata again. It also checks asset IDs/sizes/
digests did not change during validation. Only this step changes `draft` to false
and marks the release latest. Failures leave the draft unpublished.

GitHub `latest/download/latest.json` and its detached signature can temporarily
refer to different releases during CDN propagation. The client must reject that
mismatch and retry; it must never accept unsigned metadata as a fallback.

## Acceptance and bootstrap

Updater signatures authenticate update bytes. They are **not** Windows
Authenticode signatures, Apple Developer ID signatures or Apple notarization.
This pipeline does not provision those certificates or claim that SmartScreen
or Gatekeeper will trust a manually downloaded package. Any OS signing/notarization
step must finish before updater signing and metadata hashing. Never modify a
package after its updater signature is created.

Version 0.1.1 has no updater: users must install the first updater-enabled release
manually. Keeping application identifiers and installer families stable is
necessary, but unit tests do not establish upgrade compatibility. Before final
publication, validate actual old-to-new NSIS and MSI installations separately,
including per-user/elevated installation and preservation of account/settings/
Minecraft data. The native updater preserves the MSI/NSIS family.

Validate both macOS architectures on real supported systems, including writable
and protected application locations and process restart. Linux automatic
replacement is for an AppImage running from a writable AppImage location; a deb
installation shows availability and opens the fixed official releases page;
installation then uses the normal OS package installer. Non-AppImage or unwritable installations must not be treated
as successfully self-updated. Verify interrupted download, wrong signature,
current/no-update, relaunch and concurrent game/install behavior on each platform.

Local release tests use temporary synthetic payloads and newly generated
throwaway keys. They cover real signature verification, bit flips, wrong keys,
metadata substitution, exact artifact sets, duplicate fields, version/tag
bindings, CI promotion rejection and missing environment protection. They do
not prove any Windows/macOS installer ran, a live GitHub release was published,
or an end user's application updated successfully.

## Installed package migration and recovery

The settings drawer reports the installed native package family. Automatic
updates preserve it:

- Windows NSIS x64 → NSIS x64, retaining the existing per-user scope and saved
  installation location. The signed package must contain an EXE, not MSI bytes.
- Windows MSI x64 → MSI x64, retaining the per-machine installer family. The
  pinned UpgradeCode `2058b1df-56a1-51ef-bd48-d296479cd59a` is the exact value
  Tauri CLI 2.11.4 derived for the existing 0.1.1 product name; ProductCode can
  change for a major upgrade. Administrator permission may be required.
- MSI ↔ NSIS, simultaneous installations of both, renamed products, changed
  scopes and manually moved Windows installations have no automatic migration
  promise. Close the old application and use an explicit manual installer path;
  verify installed-app registrations and account/settings preservation in beta.
- macOS Intel → Intel app archive, Apple Silicon → Apple Silicon app archive.
  DMG is the bootstrap/manual distribution. Move the app out of a mounted DMG
  into an appropriate Applications directory before use. A protected destination
  may require OS permission or a manual replacement; an installation error never
  counts as a completed update.
- Linux x64 AppImage → x64 AppImage at its current writable location. Debian
  packages, RPM, bare development binaries and unsupported architectures never
  enter the self-replacement path. Install a new deb through the system package
  manager; the launcher does not run privileged package-manager commands.

These paths follow the locked [Tauri MSI implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/msi/mod.rs),
[NSIS installer template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/templates/installer.nsi)
and [updater implementation](https://github.com/tauri-apps/plugins-workspace/blob/updater-v2.11.0/plugins/updater/src/updater.rs).
Source inspection and CI packaging do not replace real installation acceptance.

The updater writes `launcher-state/pending-update.json` immediately before the
platform installer. Once handed off, cancellation, power loss and installer
errors can be ambiguous. Windows does not expose an installer PID/completion
result through the plugin, so the launcher never guesses that installation has
finished after a timeout. The exact target version acknowledges the marker on
startup. Until then, recovery mode blocks native game, account, settings and
update mutations. Corrupt marker bytes also open this diagnostic mode, never
normal operation. Its restriction stays latched even if the file is removed
while that process is open. Unix marker creation/acknowledgment syncs the parent
directory as well as file contents.

For an interrupted update:

1. Close Minecraft, all launcher instances and any installer; reboot if their
   termination cannot be established. Do not delete active OS lock files.
2. Install the marker's expected target version manually from the fixed official
   releases page, preserving the package family above. Replace only the app or
   package, keeping the application data directory. Starting that target version
   acknowledges a valid marker automatically.
3. If the marker is corrupt, or the target cannot be used, first manually restore
   a complete official package of the intended supported version. With every
   launcher/game/installer closed, rename only `launcher-state/pending-update.json`
   to a backup outside `launcher-state`, then launch again. This is an operator
   recovery after a complete package repair, not a shortcut around an active
   installer. Keep the backup for diagnosis. Never remove settings, sessions,
   profiles, game files, `instance.lock`, `writer.lock` or a live game lease.

Default application data roots (or the configured XDG data home on Linux):

- Windows: `%APPDATA%\ru.shacraft.launcher`.
- macOS: `~/Library/Application Support/ru.shacraft.launcher`.
- Linux: `~/.local/share/ru.shacraft.launcher`.

Network/hash/signature failures before installer entry leave the app intact and
permit an explicit fresh check/retry. They do not create an install handoff or
require marker recovery. A separate “game still running” error must be resolved
by closing the game, not removing its lease.


## Package architecture checks

Before collection/signing, scripts inspect package bytes without running an
installer. AppImage must have an ELF64 little-endian AMD64 header and the Type 2
`AI\x02` marker. The deb ar/control archive must declare `Architecture: amd64`
and the release version. Each macOS app archive must have exactly one
`Info.plist`, the same release version, and a regular main executable with a
thin Mach-O64 CPU type matching its matrix target. DMG checks validate the UDIF
container trailer; they do not mount or inspect the DMG filesystem.

On the Windows build runner, the built launcher must be AMD64 PE32+. NSIS uses
an x86 installer stub even for an x64 application: the checker accepts that
wrapper, uses the runner's 7-Zip to list/extract only the named launcher to
stdout, then requires an x64 payload identical to the built main executable after the
exact Tauri bundle-type stamp described below.
It never executes NSIS. Missing/unsupported 7-Zip inspection fails the build;
there is no silent architecture-check fallback. MSI is checked for a compound
file header and read using WindowsInstaller COM with `MSIDBOPEN_READONLY`:
Template Summary must say `x64` and ProductVersion must match. The MSI check also
checks the built main executable, but does not extract MSI's embedded cabinet or
prove that cabinet's payload matches the build. Real installer acceptance is
still required. These distinctions follow the [PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
and [64-bit MSI package requirements](https://learn.microsoft.com/en-us/windows/win32/msi/using-64-bit-windows-installer-packages).

The locked [Tauri CLI 2.11.4 bundler](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle.rs)
replaces the first complete `__TAURI_BUNDLE_TYPE_VAR_UNK` token with
`__TAURI_BUNDLE_TYPE_VAR_NSS` for NSIS (`MSI` for MSI), packages that binary, then
restores the original unpatched/unsigned executable on disk after each bundle.
The NSIS comparator constructs precisely that one replacement in a copy of the
built bytes and compares the entire extracted payload. It does not mask any PE
section, checksum, certificate table, padding or other bytes. A missing marker,
wrong family stamp or any unrelated byte change fails. Both original and
extracted executables must still be AMD64 PE32+.

Authenticode is currently unconfigured. Adding it can also change the PE checksum
and append a certificate table; this comparison intentionally fails until a
separate verified signed-baseline procedure is implemented. Do not broaden the
comparison to ignore all certificate/checksum differences merely to pass CI.

Portable format checks repeat when validating signed release assets. Full NSIS
payload/MSI COM checks run only during collection on the Windows runner; the
Linux draft/publish verifier rechecks their container headers and signatures.
Header/metadata inspection is not a runtime, architecture-emulation or installer
migration test. Synthetic header fixtures exercise these checks; they are never
installed or executed.
