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
