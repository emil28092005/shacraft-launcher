# ShaCraft metadata-only updater patch

This directory vendors the Rust/build portion of the official
`tauri-plugin-updater` **2.11.0** crate. Files were copied from the local Cargo
registry cache, and every copied file was checked byte-for-byte against the
cached `.crate` archive before applying this patch. No replacement crate was
downloaded for this vendoring step.

Upstream: <https://github.com/tauri-apps/plugins-workspace/tree/6aa2854f314481a459be1189b02c65a2450789ab/plugins/updater>

The published archive's SHA-256 is
`b28d8cabdeb0564f03ae261963de4bc3d98321cd3d213e76a81b7d344e5df606`.
The original registry `.cargo_vcs_info.json`, normalized `Cargo.toml` and
`Cargo.toml.orig` are retained for provenance. Unused upstream JS source, package
lockfile, changelog and Cargo cache marker are omitted; `api-iife.js` is retained
because the official build script references it.

## Local change

Only `src/updater.rs` changes upstream Rust behavior. The new public method
`Updater::check_metadata(&self, raw_json: serde_json::Value) -> Result<Option<Update>>`
is synchronous and performs **no HTTP requests**. It uses the existing release
deserializer, current-version/comparator decision, platform URL/signature
selection and `Update` construction. `Updater::check()` keeps its original
endpoint iteration and validation, then calls this same method. Its accepted
release is parsed again by the helper; endpoint selection and error semantics
are preserved.

ShaCraft fetches a size-limited metadata response and verifies its signature in
its own native trust boundary before passing that exact JSON value to this
method. This avoids asking the upstream `check()` path to fetch and parse a
second, potentially unbounded metadata response. This helper does not itself
authenticate metadata; callers must enforce their own policy.

The upstream package download, Minisign verification and all platform installer
implementations are unchanged by this patch. ShaCraft's own native integration
may choose a separate bounded download and installation path.

## License and maintenance

Upstream is dual licensed **Apache-2.0 OR MIT**. Both full license files,
`LICENSE.spdx`, copyright notices and `SECURITY.md` remain in this directory.
Rebase this small patch when upgrading the dependency, retain upstream notices,
and repeat native updater tests before release.
