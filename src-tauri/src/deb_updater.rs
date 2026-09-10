//! The only elevated updater operation. pkexec starts this installed, root-owned
//! binary in an early non-GUI mode. Input is untrusted until BOTH signatures
//! are verified again here. No user-supplied path, command or password is used.
use crate::updater::{self, InstallationKind, MAX_ARTIFACT_BYTES, MAX_METADATA_BYTES};
use serde_json::Value;
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use tauri_plugin_updater::Update;
use url::Url;

const BINARY: &str = "/usr/bin/shacraft-launcher";
const HELPER_FLAG: &str = "--shacraft-install-deb";
const PACKAGE: &str = "sha-craft-launcher";
const PKEXEC: &str = "/usr/bin/pkexec";
const DPKG: &str = "/usr/bin/dpkg";
const QUERY: &str = "/usr/bin/dpkg-query";
const DEB: &str = "/usr/bin/dpkg-deb";
const OUTPUT_LIMIT: usize = 16 * 1024;
const INPUT_MAGIC: &[u8; 8] = b"SCDUPD01";
const REJECTED: i32 = 20;
const LOCKED: i32 = 21;
const INSTALL_FAILED: i32 = 22;
const INVALID_HOST: i32 = 23;

fn architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => "unsupported",
    }
}

/// Validate the full path without following symlinks. The executable and every
/// parent must be root-owned and not writable by group/other users.
fn trusted_root_path(path: &Path, executable: bool) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    let mut leaf = true;
    for item in path.ancestors() {
        let Ok(meta) = fs::symlink_metadata(item) else {
            return false;
        };
        if meta.file_type().is_symlink() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return false;
        }
        if leaf && executable {
            if !meta.is_file() || meta.mode() & 0o111 == 0 {
                return false;
            }
        } else if !meta.is_dir() {
            return false;
        }
        leaf = false;
    }
    true
}

fn safe_sticky_temporary_parent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| {
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.uid() == 0
            && (meta.mode() & 0o022 == 0 || meta.mode() & 0o1000 != 0)
    })
}

fn fixed_command(path: &str) -> Command {
    let mut command = Command::new(path);
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C");
    command
}

fn drain_capped(mut source: impl Read) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            return Ok(result);
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(result.len());
        result.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Drain both pipes concurrently; output can never make the root helper buffer
/// unbounded data or deadlock dpkg while it is changing the package database.
fn capture(command: &mut Command) -> io::Result<Captured> {
    capture_with_deadline(command, Duration::from_secs(15))
}

fn capture_with_deadline(command: &mut Command, deadline: Duration) -> io::Result<Captured> {
    // Read-only inspection has a deadline. Never kill dpkg during mutation:
    // interrupting it could leave a partially configured installed package.
    let inspection = command.get_program() != DPKG;
    if inspection {
        command.process_group(0);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (out_send, out_receive) = mpsc::channel();
    let (err_send, err_receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = out_send.send(drain_capped(stdout));
    });
    std::thread::spawn(move || {
        let _ = err_send.send(drain_capped(stderr));
    });
    let start = Instant::now();
    let mut stdout = None;
    let mut stderr = None;
    loop {
        if stdout.is_none() {
            stdout = out_receive.try_recv().ok();
        }
        if stderr.is_none() {
            stderr = err_receive.try_recv().ok();
        }
        // Do not reap the parent before its pipes close. Its unreaped PID
        // reserves the process-group id until a possible timeout kill below.
        if stdout.is_some() && stderr.is_some() {
            if let Some(status) = child.try_wait()? {
                return Ok(Captured {
                    status,
                    stdout: stdout.unwrap()?,
                    stderr: stderr.unwrap()?,
                });
            }
        }
        if inspection && start.elapsed() > deadline {
            // Also terminate dpkg-deb's decompressor descendants so they cannot
            // retain the pipes after the inspection parent has been killed.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "package inspection timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn installed_version() -> Result<String, ()> {
    let result = capture(fixed_command(QUERY).args([
        "--show",
        "--showformat=${db:Status-Status}\n${Version}\n${Architecture}\n",
        PACKAGE,
    ]))
    .map_err(|_| ())?;
    if !result.status.success() {
        return Err(());
    }
    let fields = std::str::from_utf8(&result.stdout)
        .map_err(|_| ())?
        .lines()
        .collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != "installed" || fields[2] != architecture() {
        return Err(());
    }
    let version = semver::Version::parse(fields[1]).map_err(|_| ())?;
    if version.to_string() != fields[1] || !version.pre.is_empty() || !version.build.is_empty() {
        return Err(());
    }
    // dpkg's database must also assign the precise executable to our package.
    let owner = capture(fixed_command(QUERY).args(["--search", BINARY])).map_err(|_| ())?;
    if !owner.status.success() || owner.stdout != format!("{PACKAGE}: {BINARY}\n").as_bytes() {
        return Err(());
    }
    Ok(fields[1].to_owned())
}

pub(crate) fn installed_binary_supported() -> bool {
    std::env::current_exe().is_ok_and(|path| path == Path::new(BINARY))
        && trusted_root_path(Path::new(BINARY), true)
        && [QUERY, DEB, DPKG]
            .iter()
            .all(|path| trusted_root_path(Path::new(path), true))
        && installed_version().is_ok()
}

pub(crate) fn unsupported_reason() -> Option<String> {
    if trusted_root_path(Path::new(PKEXEC), true) {
        None
    } else {
        Some("Для обновления deb нужен системный компонент pkexec (PolicyKit). Установите его или скачайте новый deb с shacraft.ru/help#launcher.".into())
    }
}

pub(crate) fn is_deleted_installed_binary() -> bool {
    std::env::current_exe()
        .is_ok_and(|path| path == Path::new("/usr/bin/shacraft-launcher (deleted)"))
}

pub(crate) fn restart() -> Result<(), String> {
    if !trusted_root_path(Path::new(BINARY), true) {
        return Err("Установленный лаунчер недоступен. Запустите его из меню приложений.".into());
    }
    Command::new(BINARY).spawn().map_err(|_| {
        "Не удалось перезапустить лаунчер. Запустите его из меню приложений.".to_string()
    })?;
    Ok(())
}

fn write_input(mut output: impl Write, metadata: &[u8], bytes: &[u8]) -> io::Result<()> {
    if metadata.len() > MAX_METADATA_BYTES || bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "update exceeds input limit",
        ));
    }
    output.write_all(INPUT_MAGIC)?;
    output.write_all(&(metadata.len() as u64).to_be_bytes())?;
    output.write_all(metadata)?;
    output.write_all(&(bytes.len() as u64).to_be_bytes())?;
    output.write_all(bytes)
}

fn read_part(input: &mut impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let mut length = [0_u8; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);
    if length == 0 || length > maximum as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid update input length",
        ));
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_input(mut input: impl Read) -> io::Result<(Value, Vec<u8>)> {
    let mut magic = [0_u8; 8];
    input.read_exact(&mut magic)?;
    if &magic != INPUT_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid protocol",
        ));
    }
    let metadata = read_part(&mut input, MAX_METADATA_BYTES)?;
    let bytes = read_part(&mut input, MAX_ARTIFACT_BYTES)?;
    let mut trailing = [0_u8];
    if input.read(&mut trailing)? != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "trailing input"));
    }
    let raw = serde_json::from_slice(&metadata)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid metadata"))?;
    Ok((raw, bytes))
}

fn exit_message(code: Option<i32>) -> String {
    match code {
        Some(0) => "",
        Some(126) => "Установка отменена в системном окне. Текущая версия лаунчера сохранена.",
        Some(127) => "Система не разрешила установку. Подтвердите права администратора в системном окне; при его отсутствии проверьте PolicyKit.",
        Some(REJECTED) => "Системная проверка подписи или версии deb не пройдена. Установка отменена.",
        Some(LOCKED) => "Пакетный менеджер занят другой установкой. Дождитесь её завершения и нажмите «Обновить» ещё раз.",
        Some(INVALID_HOST) => "Системная установка ShaCraft не подтверждена. Установите новый deb вручную с shacraft.ru/help#launcher.",
        _ => "Пакетный менеджер не завершил установку. Проверьте состояние пакетов в системе и повторите попытку; при необходимости установите deb вручную.",
    }.to_owned()
}

pub(crate) fn install(update: &Update, bytes: &[u8]) -> Result<(), String> {
    if !installed_binary_supported() {
        return Err(exit_message(Some(INVALID_HOST)));
    }
    if let Some(reason) = unsupported_reason() {
        return Err(reason);
    }
    let metadata =
        serde_json::to_vec(&update.raw_json).map_err(|_| exit_message(Some(REJECTED)))?;
    let mut child = Command::new(PKEXEC)
        .args(["--disable-internal-agent", BINARY, HELPER_FLAG])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| exit_message(Some(127)))?;
    // Always wait even on EPIPE: declining the system dialog closes stdin, and
    // its exit status is the useful cancellation result, not "broken pipe".
    let write_result = write_input(
        child.stdin.take().expect("piped helper input"),
        &metadata,
        bytes,
    );
    let status = child.wait().map_err(|_| exit_message(None))?;
    if status.success() && write_result.is_ok() {
        Ok(())
    } else {
        Err(exit_message(status.code().filter(|code| *code != 0)))
    }
}

fn verify_deb_release(raw: &Value, bytes: &[u8], key: &str, installed: &str) -> Result<String, ()> {
    let metadata = updater::verified_metadata(raw, key).map_err(|_| ())?;
    if !updater::newer_version(&metadata, installed).map_err(|_| ())? {
        return Err(());
    }
    let version = metadata["version"].as_str().ok_or(())?;
    let target = format!("linux-{}-deb", std::env::consts::ARCH);
    let artifact = metadata["platforms"].get(&target).ok_or(())?;
    let url = Url::parse(artifact["url"].as_str().ok_or(())?).map_err(|_| ())?;
    updater::validate_download_url(&url, version, InstallationKind::Deb).map_err(|_| ())?;
    updater::verify_signature(bytes, artifact["signature"].as_str().ok_or(())?, key)
        .map_err(|_| ())?;
    Ok(version.to_owned())
}

fn valid_package_fields(output: &[u8], version: &str) -> bool {
    std::str::from_utf8(output)
        .is_ok_and(|text| text == format!("{PACKAGE}\n{version}\n{}\n", architecture()))
}

fn lock_error(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    (text.contains("lock")
        && (text.contains("locked")
            || text.contains("another process")
            || text.contains("resource temporarily unavailable")
            || text.contains("unable to acquire")))
        || text.contains("dpkg frontend lock was locked")
}

fn embedded_key() -> Result<String, ()> {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).map_err(|_| ())?;
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .map(str::to_owned)
        .ok_or(())
}

fn run_helper() -> Result<(), i32> {
    // pkexec cleans the environment before executing this root-owned program.
    // Never initialize Tauri/GTK or network/account code in privileged mode.
    if unsafe { libc::geteuid() } != 0 || !installed_binary_supported() {
        return Err(INVALID_HOST);
    }
    let installed = installed_version().map_err(|_| INVALID_HOST)?;
    let (raw, bytes) = read_input(io::stdin().lock()).map_err(|_| REJECTED)?;
    let version = verify_deb_release(
        &raw,
        &bytes,
        &embedded_key().map_err(|_| REJECTED)?,
        &installed,
    )
    .map_err(|_| REJECTED)?;
    // No untrusted filesystem object crosses the privilege boundary. This
    // directory is created by root, mode 0700, after all signature checks.
    if !trusted_root_path(Path::new("/var"), false)
        || !safe_sticky_temporary_parent(Path::new("/var/tmp"))
    {
        return Err(INVALID_HOST);
    }
    let temp = tempfile::Builder::new()
        .prefix("shacraft-update-")
        .tempdir_in("/var/tmp")
        .map_err(|_| INSTALL_FAILED)?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))
        .map_err(|_| INSTALL_FAILED)?;
    let package = temp.path().join("release.deb");
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&package)
        .map_err(|_| INSTALL_FAILED)?;
    output
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| INSTALL_FAILED)?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| INSTALL_FAILED)?;
    drop(output);
    let fields = capture(
        fixed_command(DEB)
            .arg("--show")
            .arg("--showformat=${Package}\n${Version}\n${Architecture}\n")
            .arg(&package),
    )
    .map_err(|_| REJECTED)?;
    if !fields.status.success() || !valid_package_fields(&fields.stdout, &version) {
        return Err(REJECTED);
    }
    // Check again immediately before mutation: another updater might have
    // installed the release while the authentication dialog was open.
    if !updater::newer_version(
        &serde_json::json!({"version": version}),
        &installed_version().map_err(|_| INVALID_HOST)?,
    )
    .map_err(|_| REJECTED)?
    {
        return Err(REJECTED);
    }
    let result = capture(
        fixed_command(DPKG)
            .args(["--refuse-downgrade", "--install"])
            .arg(&package),
    )
    .map_err(|_| INSTALL_FAILED)?;
    if !result.status.success() {
        return Err(if lock_error(&result.stderr) {
            LOCKED
        } else {
            INSTALL_FAILED
        });
    }
    if installed_version().map_err(|_| INSTALL_FAILED)? != version {
        return Err(INSTALL_FAILED);
    }
    Ok(())
}

/// The special flag is never registered as IPC and does not accept filenames.
/// Even manually invoking it cannot bypass signatures, package identity or
/// privilege checks. Errors intentionally print no package/metadata contents.
pub(crate) fn run_helper_if_requested() -> Option<i32> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if !arguments.iter().any(|argument| argument == HELPER_FLAG) {
        return None;
    }
    if arguments.len() != 1 || arguments[0] != HELPER_FLAG {
        return Some(REJECTED);
    }
    Some(match run_helper() {
        Ok(()) => 0,
        Err(code) => code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_input_rejects_oversized_truncated_and_trailing_data() {
        let mut bytes = Vec::new();
        write_input(&mut bytes, b"{}", b"package").unwrap();
        let (metadata, package) = read_input(&bytes[..]).unwrap();
        assert_eq!(metadata, serde_json::json!({}));
        assert_eq!(package, b"package");
        assert!(read_input(&bytes[..bytes.len() - 1]).is_err());
        bytes.push(0);
        assert!(read_input(&bytes[..]).is_err());
        let mut oversized = INPUT_MAGIC.to_vec();
        oversized.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(read_input(&oversized[..]).is_err());
    }

    #[test]
    fn package_identity_version_architecture_are_exact() {
        let good = format!("{PACKAGE}\n0.1.4\n{}\n", architecture());
        assert!(valid_package_fields(good.as_bytes(), "0.1.4"));
        for wrong in [
            good.replace(PACKAGE, "another-package"),
            good.replace("0.1.4", "0.1.5"),
            good.replace(architecture(), "all"),
            format!("{good}extra\n"),
        ] {
            assert!(!valid_package_fields(wrong.as_bytes(), "0.1.4"));
        }
    }

    #[test]
    fn cancellation_authorization_and_package_lock_remain_distinct() {
        assert!(exit_message(Some(126)).contains("отменена"));
        assert!(exit_message(Some(127)).contains("не разрешила"));
        assert!(exit_message(Some(LOCKED)).contains("занят"));
        assert!(lock_error(
            b"dpkg: error: dpkg frontend lock was locked by another process"
        ));
        assert!(!lock_error(
            b"dpkg: dependency problems prevent configuration"
        ));
    }

    #[test]
    fn privileged_path_rejects_user_owned_files_symlinks_and_relative_paths() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("launcher");
        fs::write(&file, b"file").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!trusted_root_path(&file, true));
        let link = directory.path().join("link");
        std::os::unix::fs::symlink("/usr/bin/dpkg", &link).unwrap();
        assert!(!trusted_root_path(&link, true));
        assert!(!trusted_root_path(Path::new("usr/bin/dpkg"), true));
    }

    #[test]
    fn root_verification_does_not_accept_legacy_appimage_as_deb() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/updater-signed.json")).unwrap();
        assert!(verify_deb_release(
            &fixture["metadata"],
            fixture["artifactText"].as_str().unwrap().as_bytes(),
            fixture["publicKey"].as_str().unwrap(),
            "0.0.0"
        )
        .is_err());
    }

    #[test]
    fn inspection_timeout_kills_descendants_holding_output_pipes() {
        let start = Instant::now();
        let result = capture_with_deadline(
            Command::new("/bin/sh").args(["-c", "sleep 30 & exit 0"]),
            Duration::from_millis(100),
        );
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::TimedOut));
        assert!(start.elapsed() < Duration::from_secs(3));
        let output = capture(fixed_command(DEB).arg("--version")).unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Debian"));
    }

    #[test]
    fn command_output_is_bounded_and_fully_drained() {
        let input = vec![b'x'; OUTPUT_LIMIT * 4];
        assert_eq!(drain_capped(input.as_slice()).unwrap().len(), OUTPUT_LIMIT);
    }
}
