# Signed launcher updates

The application updater is separate from the signed Aeronautics modpack
manifest. Its only metadata endpoint is
`https://shacraft.ru/launcher/updates/stable.json`. Its artifact URLs are confined
to `https://shacraft.ru/downloads/shacraft-launcher/<version>/<filename>`.
The webview cannot choose a URL, signing key or executable path. The native
updater verifies signatures before installing; an unavailable or invalid feed
does not prevent playing with the installed launcher.

The first version containing the updater must be installed manually. Version
0.1.2 has no code capable of installing this feature itself. AppImage supports
self-updates from 0.1.3; deb adds them in 0.1.4. An existing 0.1.3 deb therefore
needs one manual upgrade to 0.1.4 before its own update button can work. A deb
installation keeps its package format and asks for system administrator
authorization when installing an update. The launcher itself runs as the normal
user. Development binaries use the manual download path. Windows/macOS
publication and actual
installation tests remain separate release work; supporting a platform in the
feed schema does not certify a working release on it.

## Release-line compatibility

The archived 2026-09-09 review bundle at
`/home/emil/Desktop/shacraft-updater-review/README.md` describes a different,
unreleased updater prototype: GitHub-hosted `latest.json`, a different pinned
key and the former LoginSystem/Game Bridge proof flow. Its successful CI and
prototype version numbers do not establish compatibility with the deployed
admission protocol. The 0.1.3 and 0.1.4 releases follow the deployed 0.1.2
admission line based on `bf43254`, using the ShaCraft-hosted feed described here.

Never publish the archived `CI-NOT-FOR-RELEASE`/`CI_NOT_FOR_RELEASE` packages or
substitute the unreleased 0.2.0 prototype for an admission-compatible release.
Do not merge its updater key, endpoint or account flow blindly: that can break
both update continuity and server login. Future reconciliation requires an
explicit compatibility review retaining admission support and the key/feed
contract already distributed to players, or a separately designed migration.

## Authentication contract

The public key embedded in the application is a **dedicated Tauri updater
key**, separate from the existing modpack manifest key. Tauri's minisign format
wraps the entire minisign public-key/signature text in standard base64. The
contents of a `.sig` file belong in metadata, not its filename or URL.

The stable feed contains the normal Tauri fields `version`, `notes`, `pub_date`
and `platforms`, and two additional fields:

- `signedPayload`: standard base64 of the exact UTF-8 JSON bytes containing
  only the four normal fields. The publisher produces these bytes with sorted
  keys, compact separators, literal UTF-8 and no trailing newline.
- `metadataSignature`: the Tauri `.sig` contents for those exact payload bytes,
  signed with the same updater key that signs the application packages.

The launcher authenticates the payload, requires it to equal the visible
fields and then selects the signed platform artifact. This also authenticates
the version and artifact URL: an old signed installer cannot be relabelled as
a newer release by modifying unsigned metadata. Every artifact is separately
verified through Tauri's built-in updater signature check. The current version
must increase; there is no unsigned or automatic downgrade fallback.

The stable publisher accepts only plain `MAJOR.MINOR.PATCH` versions and these
platforms: `linux-x86_64` (legacy `.AppImage`), `linux-x86_64-appimage`
(`.AppImage`), `linux-x86_64-deb` (`.deb`), `windows-x86_64` (`.exe` or `.msi`),
`darwin-x86_64` and `darwin-aarch64` (`.app.tar.gz`). Artifact filenames contain
only ASCII letters, digits, dots, underscores and hyphens. Files must already
exist in the matching version directory, must not be symlinks and must be
between 1 byte and 256 MiB. A platform without a tested signed artifact is
omitted, never represented by an empty signature or another platform's file.

Format-aware Linux feeds must contain both AppImage keys with exactly the same
URL and signature. This keeps 0.1.3 clients on their original AppImage path.
New AppImage clients prefer `linux-x86_64-appimage` and can read the legacy key;
deb clients require `linux-x86_64-deb` and never fall back to an AppImage.
Preserve all three entries when publishing a release that supports both formats.

After verifying each signature, the publisher also checks Linux package format.
AppImage must have the ELF64 little-endian x86_64 and type-2 AppImage header.
For deb, `/usr/bin/dpkg-deb` must report package `sha-craft-launcher`, architecture
`amd64` and the exact signed release version. Inspection uses fixed arguments,
no shell, a cleared environment, a 10-second timeout and a 4 KiB output limit.
It does not install a package or execute its maintainer scripts. This protects
against accidental publication of the wrong signed package; an installer
signature remains mandatory and is checked before package inspection.

## Debian installation boundary

The installed launcher must be `/usr/bin/shacraft-launcher`, owned by root in
root-owned directories that other users cannot write. The package database
must assign that file to an installed `sha-craft-launcher` of the expected
architecture. The updater needs the system `pkexec` authorization agent; it
does not collect a password or fall back to running a shell with privileges.

After the normal-user downloader verifies the update, `pkexec` launches the
fixed installed binary with `--shacraft-install-deb`. This mode runs before
Tauri/GTK initialization. It accepts only length-bounded signed metadata and
package bytes over stdin, never a user-provided package path. The root helper
independently verifies the metadata, selects only the exact deb target, checks
the package signature and requires a higher version than the current dpkg
database. It writes the verified bytes to a root-created mode-0700 temporary
directory under the validated `/var/tmp`; the file has mode 0600.

The helper checks the package's exact name, version and architecture with
`dpkg-deb`, then invokes fixed `dpkg --refuse-downgrade --install` arguments in
an environment without inherited variables. Dpkg's own downgrade refusal
protects against a competing newer installation between the version check and
the package-manager lock. A successful result also requires the package
database to report the intended version as installed. The temporary package
is removed on completion. A signed deb may include maintainer scripts, which
dpkg runs with administrator privileges as part of normal installation: review
release package contents before signing.

Cancellation of system authorization, missing authorization support, signature
rejection, a busy package manager and installation failure have distinct
messages. There is no automatic retry with weaker checks. Dpkg installation
is not an atomic file replacement: dependency/configuration failures or power
loss can require normal package-manager recovery. The launcher reports failure
instead of claiming the old installation is intact. Successful deb updates
restart the fixed installed binary as the ordinary user. These guarantees
are separate from AppImage's same-directory atomic replacement.

## Keys and builds

The production private key stays **only on the operator's local machine** at
`/home/emil/.local/share/shacraft-updater/production.key`, with owner-only
permissions. Its public companion is `production.key.pub`. Never transfer the
private key to the web server, GitHub, CI, logs, chat, a package or a public
artifact. Signing commands below pass the local path, not the key contents.
Keep a protected operator-controlled backup: replacing or losing the key will
break continuity for installations trusting the existing public key. There is
no automatic key rotation mechanism in this release.

Normal `build.yml` jobs explicitly merge `scripts/tauri-unsigned.json` to disable
updater signing. They upload ordinary packages and unsigned macOS `.app.tar.gz`
archives. CI does not receive the production key and does not publish the
stable feed. A release operator reviews/tests these build artifacts, then signs
the chosen packages locally. For a signed local Tauri bundle build, set
`TAURI_SIGNING_PRIVATE_KEY` to the protected key path; never disable verification
in the application to make a build pass.

Updater signatures authenticate ShaCraft's update channel. They are separate
from Windows Authenticode, Apple signing/notarization, and Linux distribution
package signatures; passing updater checks does not establish those assurances.

## Local preparation and signing

The publisher requires Python 3.10+ and `minisign`; releases containing deb also
require `/usr/bin/dpkg-deb` (Debian/Ubuntu's `dpkg` package). It performs verification
through the standard minisign CLI, without implementing cryptography in Python.
`--minisign /absolute/path/to/minisign` supports a locally extracted tool without
installing a global package. Run these examples from the launcher repository,
substituting the actual release version and tested filenames.

1. Stage immutable, tested packages below a local downloads root. The following
   example assumes both tested Linux artifacts already exist below
   `/tmp/shacraft-release/downloads/0.1.4/`
   and release notes exist at `/tmp/shacraft-release/notes.txt`. Create signatures
   with the Tauri CLI; `.sig` is written beside each artifact:

   ```bash
   npm run tauri -- signer sign \
     --private-key-path /home/emil/.local/share/shacraft-updater/production.key \
     /tmp/shacraft-release/downloads/0.1.4/ShaCraft.Launcher_0.1.4_amd64.AppImage
   npm run tauri -- signer sign \
     --private-key-path /home/emil/.local/share/shacraft-updater/production.key \
     /tmp/shacraft-release/downloads/0.1.4/ShaCraft.Launcher_0.1.4_amd64.deb
   ```

2. Prepare a deterministic payload after verifying every package signature.
   Repeat `--artifact PLATFORM=FILENAME` for each tested platform included in this
   release. Keep the legacy AppImage alias. Do not list a dmg, nonexistent
   package or untested architecture:

   ```bash
   python3 scripts/publish_launcher_update.py prepare \
     --version 0.1.4 \
     --downloads-root /tmp/shacraft-release/downloads \
     --artifact linux-x86_64=ShaCraft.Launcher_0.1.4_amd64.AppImage \
     --artifact linux-x86_64-appimage=ShaCraft.Launcher_0.1.4_amd64.AppImage \
     --artifact linux-x86_64-deb=ShaCraft.Launcher_0.1.4_amd64.deb \
     --notes-file /tmp/shacraft-release/notes.txt \
     --public-key /home/emil/.local/share/shacraft-updater/production.key.pub \
     --payload /tmp/shacraft-release/release.payload.json
   ```

3. Inspect the payload and sign its exact bytes locally:

   ```bash
   npm run tauri -- signer sign \
     --private-key-path /home/emil/.local/share/shacraft-updater/production.key \
     /tmp/shacraft-release/release.payload.json
   ```

   Editing notes, timestamps, versions, signatures or URLs after this step
   invalidates the metadata signature. Prepare and sign again after any change.

## Publication

Upload **only** the packages, their `.sig` files, `release.payload.json`, its
`.sig`, the public key and the publisher script. Stage and hash-check artifacts
before publishing metadata. Production paths are:

- Downloads root: `/root/shacraft/caddy/www/downloads/shacraft-launcher`.
- Stable feed: `/root/shacraft/data/launcher/updates/stable.json`.
- Public feed: `https://shacraft.ru/launcher/updates/stable.json`.

Keep previous version directories immutable and save the current feed before
replacing it. Run the publisher on the host with a public-key file and minisign
available there. Neither operation needs a private key:

```bash
python3 publish_launcher_update.py publish \
  --downloads-root /root/shacraft/caddy/www/downloads/shacraft-launcher \
  --public-key /path/to/production.key.pub \
  --payload /path/to/release.payload.json \
  --signature /path/to/release.payload.json.sig \
  --output /root/shacraft/data/launcher/updates/stable.json \
  --dry-run
```

After that succeeds, repeat without `--dry-run`. The publisher verifies metadata
and all artifacts under the public key, authenticates the previous feed before
comparing versions, and refuses same-version replacement or downgrade. It holds
an exclusive publication lock and writes/fsyncs a temporary sibling before
atomically replacing `stable.json`. Dry-run validates everything but does not
replace the feed. Do not change staged artifacts concurrently with publication.
Do not overwrite a released version to add another platform: publish a higher
version containing the complete intended platform set.

Alternatively, run the same verification locally against byte-for-byte copies
of the current feed and staged downloads, then deploy the resulting feed only
after checking uploaded package and metadata hashes against those validated
files. An initial publication has no previous feed; subsequent publications
must validate against the actual deployed feed, not an empty staging directory.

Caddy should serve this feed as JSON with `Cache-Control: no-store`. Check the
public response, decoded metadata, signatures and downloadable artifact hashes
after deployment. Exercise a real installed AppImage updating to a higher
version, including relaunch and retained settings/account state. For deb, also
exercise administrator cancellation, package-manager lock conflicts, failed
installation and a successful package upgrade/relaunch. Use an isolated system
for destructive package-manager failure cases; never modify player data as a
test fixture. Unit tests, packaging or a browser mock alone do not establish
successful installation or distribution compatibility.
If a release is faulty, stop offering it and publish a corrected higher version;
do not weaken signature checks or silently downgrade users.

## Verification

```bash
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Publisher tests exercise the real minisign CLI with temporary test keys,
including valid publication, modified packages and metadata, authenticated
previous-version checks, downgrade refusal, URL/path restrictions and dry-run.
Linux tests also build real temporary deb packages with `dpkg-deb`, validate
package/version/architecture, ensure inspection never executes maintainer
scripts, retain the legacy AppImage feed alias, and reject malformed signed
Linux packages. Package-inspection output and time bounds are exercised.
No test private key is checked into the repository. CI installs minisign so
the signature tests run; locally they explicitly skip if the tool is absent.
Set `SHACRAFT_TEST_MINISIGN` to use an extracted executable.

The artifact formats and signature encoding follow the
[official Tauri updater documentation](https://v2.tauri.app/plugin/updater/).
