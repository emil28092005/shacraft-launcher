//! Local provenance for outputs created by the verified official installer.
//! No receipt is ever bootstrapped by hashing an unknown legacy installation.
//! The receipt is a final commit marker, not a vendor signature for output jars.
use super::{ensure_launcher_profiles_stub, installed_version_json_path, NeoForgeError};
use crate::{download, manifest::is_portable_component, mojang::VersionJson, storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_METADATA: u64 = 4 * 1024 * 1024;
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

fn invalid(message: impl Into<String>) -> NeoForgeError {
    NeoForgeError::InvalidInstallation(message.into())
}

/// Refuse links in every existing ancestor, including launcher root ancestors.
/// Same-user concurrent path substitution remains outside the OS trust model.
fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, NeoForgeError> {
    if relative.is_empty() || !relative.split('/').all(is_portable_component) {
        return Err(invalid("unsafe NeoForge artifact path"));
    }
    let target = root.join(relative);
    for path in target.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid(format!(
                    "symlink in NeoForge path: {}",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(target)
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, NeoForgeError> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err(invalid("NeoForge metadata is too large"));
    }
    Ok(bytes)
}

fn embedded(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<Vec<u8>, NeoForgeError> {
    let entry = archive
        .by_name(name)
        .map_err(|error| invalid(error.to_string()))?;
    let mut bytes = Vec::new();
    entry.take(MAX_METADATA + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err(invalid("embedded NeoForge metadata is too large"));
    }
    Ok(bytes)
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && is_portable_component(value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}

/// Only provider-owned Maven outputs are published. Archive paths, absolute
/// arguments, ROOT substitutions and client-controlled filenames are rejected.
fn coordinate_path(coordinate: &str) -> Result<String, NeoForgeError> {
    let coordinate = coordinate
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| invalid("unsupported NeoForge output coordinate"))?;
    let (coordinate, extension) = coordinate.split_once('@').unwrap_or((coordinate, "jar"));
    let parts: Vec<_> = coordinate.split(':').collect();
    if !(3..=4).contains(&parts.len())
        || !parts.iter().all(|s| valid_version(s))
        || !matches!(parts[0], "net.minecraft" | "net.neoforged")
        || !matches!(extension, "jar" | "txt")
    {
        return Err(invalid("unsupported NeoForge output coordinate"));
    }
    let classifier = parts.get(3).map(|v| format!("-{v}")).unwrap_or_default();
    let relative = format!(
        "libraries/{}/{}/{}/{}-{}{classifier}.{extension}",
        parts[0].replace('.', "/"),
        parts[1],
        parts[2],
        parts[1],
        parts[2]
    );
    if !relative.split('/').all(is_portable_component) {
        return Err(invalid("unsafe generated NeoForge coordinate"));
    }
    Ok(relative)
}

fn data_value<'a>(data: &'a Value, argument: &'a str) -> Result<&'a str, NeoForgeError> {
    if let Some(key) = argument.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        data.get(key)
            .and_then(|v| v.get("client"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("missing client recipe value: {key}")))
    } else {
        Ok(argument)
    }
}

struct Recipe {
    version_bytes: Vec<u8>,
    version_relative: String,
    outputs: BTreeMap<String, Option<String>>,
    identity: Identity,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Identity {
    schema: u32,
    loader: String,
    minecraft: String,
    installer_sha256: String,
    recipe_sha256: String,
    vanilla_sha1: String,
    vanilla_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    identity: Identity,
    files: BTreeMap<String, FileDigest>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileDigest {
    size: u64,
    sha256: String,
}

impl Recipe {
    fn read(installer: &Path, loader: &str, vanilla: &VersionJson) -> Result<Self, NeoForgeError> {
        if !valid_version(loader) || !valid_version(&vanilla.id) {
            return Err(invalid("invalid Minecraft or NeoForge version"));
        }
        let mut archive = zip::ZipArchive::new(fs::File::open(installer)?)
            .map_err(|error| invalid(error.to_string()))?;
        let profile_bytes = embedded(&mut archive, "install_profile.json")?;
        let version_bytes = embedded(&mut archive, "version.json")?;
        let profile: Value =
            serde_json::from_slice(&profile_bytes).map_err(NeoForgeError::InvalidJson)?;
        let version: Value =
            serde_json::from_slice(&version_bytes).map_err(NeoForgeError::InvalidJson)?;
        let id = format!("neoforge-{loader}");
        if profile["spec"] != 1
            || profile["version"] != id
            || profile["minecraft"] != vanilla.id
            || profile["json"] != "/version.json"
            || version["id"] != id
            || version["inheritsFrom"] != vanilla.id
        {
            return Err(invalid(
                "installer recipe does not match selected Minecraft/NeoForge",
            ));
        }
        serde_json::from_slice::<VersionJson>(&version_bytes)
            .map_err(NeoForgeError::InvalidJson)?;
        let data = &profile["data"];
        let processors = profile["processors"]
            .as_array()
            .ok_or_else(|| invalid("missing processors"))?;
        let mut outputs = BTreeMap::new();
        for processor in processors {
            if let Some(sides) = processor.get("sides") {
                let sides = sides
                    .as_array()
                    .ok_or_else(|| invalid("invalid processor sides"))?;
                if !sides.iter().any(|side| side == "client") {
                    continue;
                }
            }
            let args = processor["args"]
                .as_array()
                .ok_or_else(|| invalid("missing processor args"))?;
            for pair in args.windows(2) {
                if matches!(pair[0].as_str(), Some("--output" | "--slim" | "--extra")) {
                    let argument = pair[1]
                        .as_str()
                        .ok_or_else(|| invalid("invalid output argument"))?;
                    let path = coordinate_path(data_value(data, argument)?)?;
                    outputs.entry(path).or_insert(None);
                }
            }
            if let Some(expected) = processor.get("outputs") {
                for (argument, digest) in expected
                    .as_object()
                    .ok_or_else(|| invalid("invalid processor outputs"))?
                {
                    let path = coordinate_path(data_value(data, argument)?)?;
                    let digest = data_value(
                        data,
                        digest
                            .as_str()
                            .ok_or_else(|| invalid("invalid output hash"))?,
                    )?;
                    let digest = digest
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                        .unwrap_or(digest);
                    if digest.len() != 40 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(invalid("unsupported processor output checksum"));
                    }
                    if let Some(Some(existing)) = outputs.get(&path) {
                        if existing != &digest.to_ascii_lowercase() {
                            return Err(invalid("conflicting output hashes"));
                        }
                    }
                    outputs.insert(path, Some(digest.to_ascii_lowercase()));
                }
            }
        }
        let mut portable_paths = BTreeSet::new();
        if outputs
            .keys()
            .any(|path| !portable_paths.insert(path.to_ascii_lowercase()))
        {
            return Err(invalid(
                "generated output paths collide on a case-insensitive filesystem",
            ));
        }
        let patched = coordinate_path(data_value(data, "{PATCHED}")?)?;
        let expected_patched =
            format!("libraries/net/neoforged/neoforge/{loader}/neoforge-{loader}-client.jar");
        let extra = coordinate_path(data_value(data, "{MC_EXTRA}")?)?;
        if patched != expected_patched
            || !outputs.contains_key(&patched)
            || !outputs.contains_key(&extra)
        {
            return Err(invalid(
                "unsupported recipe: missing patched client or extra output",
            ));
        }
        let client = &vanilla
            .downloads
            .as_ref()
            .ok_or_else(|| invalid("missing verified vanilla download"))?
            .client;
        if client.size == 0
            || client.sha1.len() != 40
            || !client.sha1.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("invalid verified vanilla identity"));
        }
        Ok(Self {
            version_relative: format!("versions/{id}/{id}.json"),
            version_bytes,
            outputs,
            identity: Identity {
                schema: 1,
                loader: loader.into(),
                minecraft: vanilla.id.clone(),
                installer_sha256: download::file_hashes(installer)?.1,
                recipe_sha256: hash_bytes(&profile_bytes),
                vanilla_sha1: client.sha1.to_ascii_lowercase(),
                vanilla_size: client.size,
            },
        })
    }

    fn paths(&self) -> impl Iterator<Item = &String> {
        self.outputs
            .keys()
            .chain(std::iter::once(&self.version_relative))
    }

    fn current(&self, root: &Path, receipt_path: &Path) -> Result<bool, NeoForgeError> {
        let receipt = match bounded_read(receipt_path) {
            Ok(bytes) => match serde_json::from_slice::<Receipt>(&bytes) {
                Ok(receipt) => receipt,
                Err(_) => return Ok(false),
            },
            Err(NeoForgeError::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(NeoForgeError::InvalidInstallation(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        if receipt.identity != self.identity || receipt.files.len() != self.outputs.len() + 1 {
            return Ok(false);
        }
        // Enumerate trusted recipe paths, never paths claimed by the local receipt.
        for relative in self.paths() {
            let Some(digest) = receipt.files.get(relative) else {
                return Ok(false);
            };
            let path = safe_path(root, relative)?;
            if !download::is_current(
                &path,
                Some(digest.size),
                &download::Checksum::Sha256(digest.sha256.clone()),
            )? {
                return Ok(false);
            }
        }
        Ok(bounded_read(&safe_path(root, &self.version_relative)?)? == self.version_bytes)
    }
}

struct Stage(PathBuf);
impl Stage {
    fn new(cache: &Path) -> Result<Self, NeoForgeError> {
        for _ in 0..128 {
            let name = format!(
                "neoforge-stage-{}-{}",
                std::process::id(),
                NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
            );
            let path = safe_path(cache, &name)?;
            fs::create_dir_all(cache)?;
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(invalid("cannot create NeoForge staging directory"))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn atomic_copy(source: &Path, target: &Path) -> Result<(), NeoForgeError> {
    let mut source = fs::File::open(source)?;
    let mut output = storage::AtomicFile::new(target)?;
    io::copy(&mut source, output.writer())?;
    output.commit()?;
    Ok(())
}

fn validate_output(path: &Path, expected_sha1: Option<&str>) -> Result<FileDigest, NeoForgeError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(invalid("empty generated artifact"));
    }
    if path.extension().is_some_and(|ext| ext == "jar") {
        let mut archive = zip::ZipArchive::new(fs::File::open(path)?)
            .map_err(|error| invalid(error.to_string()))?;
        if archive.is_empty() {
            return Err(invalid("empty generated jar"));
        }
        // Reading every entry validates ZIP checksums, not just its directory.
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| invalid(error.to_string()))?;
            io::copy(&mut entry, &mut io::sink())?;
        }
    }
    let (sha1, sha256) = download::file_hashes(path)?;
    if expected_sha1.is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha1)) {
        return Err(invalid(
            "generated artifact differs from recipe output checksum",
        ));
    }
    Ok(FileDigest {
        size: metadata.len(),
        sha256,
    })
}

/// `installer` is supplied only by ensure_installer, after fixed-host SHA-256
/// verification. Tests inject a synthetic archive and a bounded fake runner.
/// A failed promotion has no receipt; next invocation rebuilds from scratch.
pub(super) fn ensure(
    installer: &Path,
    game: &Path,
    cache: &Path,
    loader: &str,
    vanilla: &VersionJson,
    run: impl FnOnce(&Path) -> Result<(), NeoForgeError>,
) -> Result<VersionJson, NeoForgeError> {
    let recipe = Recipe::read(installer, loader, vanilla)?;
    let receipt_path = safe_path(cache, &format!("neoforge-receipts/{loader}.json"))?;
    if recipe.current(game, &receipt_path)? {
        return serde_json::from_slice(&recipe.version_bytes).map_err(NeoForgeError::InvalidJson);
    }
    // Invalidate before changing any output. Even interruption during multi-file
    // promotion cannot leave a complete receipt over a partial installation.
    match fs::remove_file(&receipt_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Validate all destination paths before invoking the installer.
    for relative in recipe.paths() {
        safe_path(game, relative)?;
    }
    let stage = Stage::new(cache)?;
    ensure_launcher_profiles_stub(&stage.0)?;
    let vanilla_relative = format!("versions/{0}/{0}.jar", vanilla.id);
    let source = safe_path(game, &vanilla_relative)?;
    let input = safe_path(&stage.0, &vanilla_relative)?;
    atomic_copy(&source, &input)?;
    if !download::is_current(
        &input,
        Some(recipe.identity.vanilla_size),
        &download::Checksum::Sha1(recipe.identity.vanilla_sha1.clone()),
    )? {
        return Err(invalid(
            "vanilla input changed or was not verified before NeoForge installation",
        ));
    }
    run(&stage.0)?;
    let version = safe_path(&stage.0, &recipe.version_relative)?;
    if bounded_read(&version)? != recipe.version_bytes {
        return Err(invalid("installer produced unexpected version JSON"));
    }
    let mut files = BTreeMap::new();
    for (relative, sha1) in &recipe.outputs {
        files.insert(
            relative.clone(),
            validate_output(&safe_path(&stage.0, relative)?, sha1.as_deref())?,
        );
    }
    files.insert(
        recipe.version_relative.clone(),
        FileDigest {
            size: recipe.version_bytes.len() as u64,
            sha256: hash_bytes(&recipe.version_bytes),
        },
    );
    for relative in recipe.paths() {
        atomic_copy(&safe_path(&stage.0, relative)?, &safe_path(game, relative)?)?;
    }
    let receipt = Receipt {
        identity: recipe.identity,
        files,
    };
    storage::write_atomic(
        &receipt_path,
        &serde_json::to_vec(&receipt).map_err(NeoForgeError::InvalidJson)?,
    )?;
    // Use the same expected bytes for merge as for receipt validation.
    debug_assert_eq!(
        installed_version_json_path(game, loader),
        game.join(&recipe.version_relative)
    );
    serde_json::from_slice(&recipe.version_bytes).map_err(NeoForgeError::InvalidJson)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        io::{Cursor, Write},
    };
    use zip::write::SimpleFileOptions;

    const LOADER: &str = "21.1.248";

    fn jar_bytes(contents: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("fixture.class", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents).unwrap();
        zip.finish().unwrap().into_inner()
    }

    struct Fixture {
        _root: Stage,
        installer: PathBuf,
        game: PathBuf,
        cache: PathBuf,
        vanilla: VersionJson,
        profile: Value,
        version: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Self {
            // macOS temporary directories may use the system /var -> /private/var
            // alias. Canonicalize only this trusted fixture anchor; production
            // safe_path must continue rejecting root/descendant substitutions.
            let temporary_root = std::env::temp_dir().canonicalize().unwrap();
            let root = Stage::new(&temporary_root).unwrap();
            let game = root.0.join("game");
            let cache = root.0.join("cache");
            let input = jar_bytes(b"verified vanilla");
            let input_path = game.join("versions/1.21.1/1.21.1.jar");
            fs::create_dir_all(input_path.parent().unwrap()).unwrap();
            fs::write(&input_path, &input).unwrap();
            let vanilla = serde_json::from_value(serde_json::json!({
                "id":"1.21.1", "mainClass":"Main", "downloads":{"client":{
                    "sha1":download::file_hashes(&input_path).unwrap().0,
                    "size":input.len(), "url":"https://piston-data.mojang.com/client.jar"
                }}
            }))
            .unwrap();
            let profile = serde_json::json!({
                "spec":1, "version":"neoforge-21.1.248", "minecraft":"1.21.1", "json":"/version.json",
                "data":{
                    "PATCHED":{"client":"[net.neoforged:neoforge:21.1.248:client]"},
                    "MC_EXTRA":{"client":"[net.minecraft:client:1.21.1-20240808.144430:extra]"},
                    "MAPPINGS":{"client":"[net.neoforged:neoform:1.21.1-20240808.144430:mappings@txt]"}
                },
                "processors":[
                    {"sides":["server"],"args":["--output","{ROOT}/run.sh"]},
                    {"args":["--output","{MAPPINGS}"]},
                    {"sides":["client"],"args":["--extra","{MC_EXTRA}"]},
                    {"args":["--output","{PATCHED}"]}
                ], "libraries":[]
            });
            let version = serde_json::to_vec(&serde_json::json!({
                "id":"neoforge-21.1.248", "inheritsFrom":"1.21.1", "mainClass":"Main", "libraries":[]
            })).unwrap();
            let fixture = Self {
                installer: root.0.join("installer.jar"),
                _root: root,
                game,
                cache,
                vanilla,
                profile,
                version,
            };
            fixture.write_installer();
            fixture
        }

        fn write_installer(&self) {
            let mut archive = zip::ZipWriter::new(fs::File::create(&self.installer).unwrap());
            for (name, bytes) in [
                (
                    "install_profile.json",
                    serde_json::to_vec(&self.profile).unwrap(),
                ),
                ("version.json", self.version.clone()),
            ] {
                archive
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                archive.write_all(&bytes).unwrap();
            }
            archive.finish().unwrap();
        }

        fn receipt(&self) -> PathBuf {
            self.cache.join("neoforge-receipts/21.1.248.json")
        }
        fn patched(&self) -> PathBuf {
            self.game
                .join("libraries/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-client.jar")
        }
        fn run(
            &self,
            runner: impl FnOnce(&Path) -> Result<(), NeoForgeError>,
        ) -> Result<VersionJson, NeoForgeError> {
            ensure(
                &self.installer,
                &self.game,
                &self.cache,
                LOADER,
                &self.vanilla,
                runner,
            )
        }
        fn produce(&self, stage: &Path) -> Result<(), NeoForgeError> {
            let recipe = Recipe::read(&self.installer, LOADER, &self.vanilla)?;
            for relative in recipe.outputs.keys() {
                let path = stage.join(relative);
                assert!(
                    !path.exists(),
                    "must never reuse old generated files in staging"
                );
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(
                    &path,
                    if relative.ends_with(".jar") {
                        jar_bytes(b"clean generated output")
                    } else {
                        b"mappings".to_vec()
                    },
                )?;
            }
            let version = stage.join(recipe.version_relative);
            fs::create_dir_all(version.parent().unwrap())?;
            fs::write(version, &self.version)?;
            Ok(())
        }
    }

    #[test]
    fn legacy_is_rebuilt_and_healthy_receipt_skips_runner() {
        let f = Fixture::new();
        fs::create_dir_all(f.patched().parent().unwrap()).unwrap();
        fs::write(f.patched(), b"legacy corrupt nonempty jar").unwrap();
        let profile_file = f._root.0.join("profiles/aeronautics/mods/user.jar");
        fs::create_dir_all(profile_file.parent().unwrap()).unwrap();
        fs::write(&profile_file, b"user mod").unwrap();
        f.run(|stage| f.produce(stage)).unwrap();
        assert!(f.receipt().exists());
        assert_ne!(
            fs::read(f.patched()).unwrap(),
            b"legacy corrupt nonempty jar"
        );
        f.run(|_| panic!("healthy receipt must not run installer"))
            .unwrap();
        assert_eq!(fs::read(profile_file).unwrap(), b"user mod");
    }

    #[test]
    fn nonempty_json_and_jar_corruption_trigger_clean_rebuild() {
        let f = Fixture::new();
        f.run(|stage| f.produce(stage)).unwrap();
        for bytes in [
            b"broken json".as_slice(),
            br#"{"id":"neoforge-21.1.248","mainClass":"Wrong"}"#,
        ] {
            fs::write(installed_version_json_path(&f.game, LOADER), bytes).unwrap();
            let called = Cell::new(false);
            f.run(|stage| {
                called.set(true);
                f.produce(stage)
            })
            .unwrap();
            assert!(called.get());
        }
        for bytes in [
            b"not a zip".to_vec(),
            jar_bytes(b"changed but valid zip"),
            Vec::new(),
        ] {
            fs::write(f.patched(), bytes).unwrap();
            let called = Cell::new(false);
            f.run(|stage| {
                called.set(true);
                f.produce(stage)
            })
            .unwrap();
            assert!(called.get());
        }
        fs::remove_file(f.patched()).unwrap();
        f.run(|stage| f.produce(stage)).unwrap();
    }

    #[test]
    fn corrupt_vanilla_input_is_rejected_before_processors() {
        let f = Fixture::new();
        fs::write(f.game.join("versions/1.21.1/1.21.1.jar"), b"corrupt input").unwrap();
        assert!(f
            .run(|_| panic!("must verify vanilla before running processors"))
            .is_err());
        assert!(!f.receipt().exists());
    }

    #[test]
    fn failed_runner_and_invalid_outputs_do_not_create_receipt() {
        let f = Fixture::new();
        assert!(f
            .run(|_| Err(invalid("simulated installer failure")))
            .is_err());
        assert!(!f.receipt().exists());
        assert!(f
            .run(|stage| {
                f.produce(stage)?;
                fs::write(
                    stage.join(
                        "libraries/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-client.jar",
                    ),
                    b"nonempty damaged jar",
                )?;
                Ok(())
            })
            .is_err());
        assert!(!f.receipt().exists());
        f.run(|stage| f.produce(stage)).unwrap();
    }

    #[test]
    fn missing_commit_marker_after_partial_promotion_forces_rebuild() {
        let f = Fixture::new();
        f.run(|stage| f.produce(stage)).unwrap();
        // Equivalent persisted state to interruption after one promoted file.
        fs::remove_file(f.receipt()).unwrap();
        fs::write(f.patched(), jar_bytes(b"partially promoted generation")).unwrap();
        let called = Cell::new(false);
        f.run(|stage| {
            called.set(true);
            f.produce(stage)
        })
        .unwrap();
        assert!(called.get());
        f.run(|_| panic!("recovered generation must be complete"))
            .unwrap();
    }

    #[test]
    fn receipt_cannot_invent_output_paths_and_changed_recipe_rebuilds() {
        let mut f = Fixture::new();
        f.run(|stage| f.produce(stage)).unwrap();
        let mut receipt: Value = serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
        receipt["files"]["../../user.jar"] = serde_json::json!({"size":1,"sha256":"00"});
        fs::write(f.receipt(), serde_json::to_vec(&receipt).unwrap()).unwrap();
        f.run(|stage| f.produce(stage)).unwrap();
        f.profile["recipeChange"] = Value::Bool(true);
        f.write_installer();
        let called = Cell::new(false);
        f.run(|stage| {
            called.set(true);
            f.produce(stage)
        })
        .unwrap();
        assert!(called.get());
    }

    #[test]
    fn recipe_version_output_paths_and_authoritative_hashes_are_enforced() {
        let mut f = Fixture::new();
        f.profile["minecraft"] = Value::String("1.20.1".into());
        f.write_installer();
        assert!(f.run(|_| panic!("wrong version recipe")).is_err());
        f.profile["minecraft"] = Value::String("1.21.1".into());
        f.profile["data"]["PATCHED"]["client"] =
            Value::String("[net.neoforged:neoforge:../escape:client]".into());
        f.write_installer();
        assert!(f.run(|_| panic!("escaping output")).is_err());
        f.profile["data"]["PATCHED"]["client"] =
            Value::String("[net.neoforged:neoforge:21.1.248:client]".into());
        f.profile["processors"][3]["outputs"] =
            serde_json::json!({"{PATCHED}":"0000000000000000000000000000000000000000"});
        f.write_installer();
        assert!(f.run(|stage| f.produce(stage)).is_err());
        assert!(!f.receipt().exists());
    }

    #[cfg(unix)]
    #[test]
    fn output_symlinks_are_rejected_without_touching_target() {
        let f = Fixture::new();
        let outside = f._root.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("user"), b"untouched").unwrap();
        std::os::unix::fs::symlink(&outside, f.game.join("libraries")).unwrap();
        assert!(f
            .run(|_| panic!("symlink rejected before installer"))
            .is_err());
        assert_eq!(fs::read(outside.join("user")).unwrap(), b"untouched");
        assert!(!f.receipt().exists());
    }
}
