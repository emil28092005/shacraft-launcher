#!/usr/bin/env python3
"""Assemble/verify release assets. No network or publication in this module."""

import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from datetime import datetime
from pathlib import Path

import release_formats

REPOSITORY = "emil28092005/shacraft-launcher"
ORIGIN = f"https://github.com/{REPOSITORY}/releases/download"
PLATFORMS = {
    "windows-x86_64": ("nsis/*.exe", "-setup.exe"),
    "linux-x86_64": ("appimage/*.AppImage", ".AppImage"),
    "darwin-aarch64": ("macos/*.app.tar.gz", ".app.tar.gz"),
    "darwin-x86_64": ("macos/*.app.tar.gz", ".app.tar.gz"),
}
MANUAL = {
    "windows-x86_64-msi": ("windows-x86_64", "msi/*.msi", ".msi"),
    "linux-x86_64-deb": ("linux-x86_64", "deb/*.deb", ".deb"),
    "darwin-aarch64-dmg": ("darwin-aarch64", "dmg/*.dmg", ".dmg"),
    "darwin-x86_64-dmg": ("darwin-x86_64", "dmg/*.dmg", ".dmg"),
}
FIELDS = {"url", "signature", "sha256", "size"}
MAX_SIZE = 1024**3


def require(condition, message):
    if not condition:
        raise ValueError(message)


def version_tag(version, tag):
    require(
        re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version),
        "release version must be stable MAJOR.MINOR.PATCH",
    )
    major, minor, patch = map(int, version.split("."))
    require(major <= 255 and minor <= 255 and patch <= 65535, "version exceeds MSI limits")
    require(tag == f"v{version}", "tag/version mismatch")


def filename(version, platform, suffix):
    return f"shacraft-launcher_{version}_{platform}{suffix}"


def expected_names(version):
    return {filename(version, key, value[1]) for key, value in PLATFORMS.items()} | {
        filename(version, platform, suffix) for platform, _, suffix in MANUAL.values()
    }


def canonical(data):
    return (json.dumps(data, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def read_json(path):
    return json.loads(path.read_bytes(), object_pairs_hook=unique_object)


def public_key():
    value = os.environ.get("SHACRAFT_UPDATER_PUBLIC_KEY", "").strip()
    require(bool(value), "SHACRAFT_UPDATER_PUBLIC_KEY is missing")
    try:
        decoded = base64.b64decode(value, validate=True).decode("utf-8")
        lines = decoded.splitlines()
        raw = base64.b64decode(lines[1], validate=True)
        require(
            lines[0].startswith("untrusted comment:") and len(raw) == 42 and raw[:2] == b"Ed",
            "invalid Tauri public key",
        )
    except (ValueError, IndexError, UnicodeError) as error:
        raise ValueError("invalid Tauri public key") from error
    return value


def verifier_path(root):
    return os.environ.get(
        "RELEASE_VERIFIER",
        str(
            root
            / "scripts/release-verifier/target/release"
            / ("shacraft-release-verifier.exe" if os.name == "nt" else "shacraft-release-verifier")
        ),
    )


def verify_signature(root, data, signature):
    with tempfile.TemporaryDirectory(prefix="shacraft-verify-") as temporary:
        key = Path(temporary) / "public-key"
        key.write_text(public_key(), encoding="utf-8")
        result = subprocess.run(
            [verifier_path(root), str(key), str(data), str(signature)],
            capture_output=True,
            timeout=120,
            check=False,
        )
        require(result.returncode == 0, f"invalid updater signature: {data.name}")


def signer(root, data):
    private = os.environ.get("TAURI_SIGNING_PRIVATE_KEY", "")
    require(bool(private.strip()), "TAURI_SIGNING_PRIVATE_KEY is missing")
    environment = dict(os.environ)
    # An omitted password means an unencrypted key. Never allow a CI prompt;
    # encrypted keys without the correct password fail in the signer.
    environment.setdefault("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "")
    # Builds accept a key path; signer sign accepts the encoded key contents.
    try:
        is_key_path = len(private) < 4096 and "\n" not in private and Path(private).is_file()
    except OSError:
        is_key_path = False
    if is_key_path:
        environment["TAURI_SIGNING_PRIVATE_KEY"] = Path(private).read_text().strip()
    environment.pop("TAURI_SIGNING_PRIVATE_KEY_PATH", None)
    result = subprocess.run(
        ["node", str(root / "node_modules/@tauri-apps/cli/tauri.js"), "signer", "sign", str(data)],
        env=environment,
        capture_output=True,
        timeout=120,
        check=False,
    )
    require(result.returncode == 0, "Tauri signing failed (check protected signing credentials)")


def preflight(root, version, tag, check_git=False, signing=False):
    version_tag(version, tag)
    versions = [
        read_json(root / "package.json")["version"],
        read_json(root / "package-lock.json")["version"],
        read_json(root / "package-lock.json")["packages"][""]["version"],
        read_json(root / "src-tauri/tauri.conf.json")["version"],
        tomllib.loads((root / "src-tauri/Cargo.toml").read_text())["package"]["version"],
    ]
    require(all(value == version for value in versions), "source versions disagree with release")
    if check_git:

        def git(*args):
            return subprocess.check_output(["git", *args], cwd=root, text=True).strip()

        require(
            git("rev-parse", "HEAD") == git("rev-parse", f"refs/tags/{tag}^{{commit}}"),
            "checkout does not match existing release tag",
        )
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], cwd=root, check=True
        )
    if signing:
        require(os.environ.get("SHACRAFT_UPDATER_TEST_BUILD") != "1", "CI test keys cannot release")
        pinned = (root / "src-tauri/updater-public-key.txt").read_text().strip()
        require(public_key() == pinned, "release public key differs from committed updater key")
        with tempfile.TemporaryDirectory(prefix="shacraft-key-check-") as temporary:
            challenge = Path(temporary) / "key-check"
            challenge.write_bytes(f"ShaCraft release key check {tag}\n".encode())
            signer(root, challenge)
            verify_signature(root, challenge, Path(str(challenge) + ".sig"))


def write_build_config(destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(
        canonical(
            {
                "bundle": {"createUpdaterArtifacts": True},
                "plugins": {"updater": {"pubkey": public_key()}},
            }
        )
    )


def ci_key(root, directory):
    require(not os.environ.get("TAURI_SIGNING_PRIVATE_KEY"), "CI must not receive production key")
    directory.mkdir(parents=True, exist_ok=True)
    key = directory / "DISPOSABLE-CI-ONLY.key"
    result = subprocess.run(
        [
            "node",
            str(root / "node_modules/@tauri-apps/cli/tauri.js"),
            "signer",
            "generate",
            "--ci",
            "--password",
            "",
            "--write-keys",
            str(key),
        ],
        capture_output=True,
        timeout=120,
        check=False,
    )
    require(result.returncode == 0, "disposable CI key generation failed")
    key.chmod(0o600)
    values = {
        "TAURI_SIGNING_PRIVATE_KEY": str(key),
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "",
        "SHACRAFT_UPDATER_PUBLIC_KEY": Path(str(key) + ".pub").read_text().strip(),
        "SHACRAFT_UPDATER_TEST_BUILD": "1",
    }
    os.environ.update(values)
    write_build_config(directory / "updater-build.json")
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as stream:
        for name, value in values.items():
            require("\n" not in value and "\r" not in value, "invalid CI environment value")
            stream.write(f"{name}={value}\n")


def collect(root, bundle, destination, platform, version):
    version_tag(version, f"v{version}")
    destination.mkdir(parents=True, exist_ok=True)
    require(not list(destination.iterdir()), "collection destination must be empty")
    entries = [(PLATFORMS[platform][0], PLATFORMS[platform][1], True)]
    entries += [
        (glob, suffix, suffix == ".msi") for key, glob, suffix in MANUAL.values() if key == platform
    ]
    for glob, suffix, required_signature in entries:
        matches = list(bundle.glob(glob))
        require(
            len(matches) == 1 and matches[0].is_file() and not matches[0].is_symlink(),
            f"expected exactly one {platform} {glob}",
        )
        source = matches[0]
        main = bundle.parent / "shacraft-launcher.exe" if platform == "windows-x86_64" else None
        release_formats.validate(source, platform, suffix, version, main)
        target = destination / filename(version, platform, suffix)
        shutil.copyfile(source, target)
        source_signature = Path(str(source) + ".sig")
        signature = Path(str(target) + ".sig")
        if required_signature:
            require(
                source_signature.is_file() and not source_signature.is_symlink(),
                f"missing generated updater signature: {source.name}",
            )
            shutil.copyfile(source_signature, signature)
        else:
            signer(root, target)  # Sign manual packages explicitly, independent of bundler output.
        verify_signature(root, target, signature)
    if os.environ.get("SHACRAFT_UPDATER_TEST_BUILD") == "1":
        (destination / "CI_NOT_FOR_RELEASE.txt").write_text(
            "DISPOSABLE TEST KEY. These CI artifacts are not deployable releases.\n",
            encoding="utf-8",
        )


def descriptor(root, directory, version, tag, platform, suffix):
    name = filename(version, platform, suffix)
    path, sig = directory / name, directory / (name + ".sig")
    require(
        path.is_file() and not path.is_symlink() and sig.is_file() and not sig.is_symlink(),
        f"missing or unsafe release asset: {name}",
    )
    size = path.stat().st_size
    require(0 < size <= MAX_SIZE, "invalid package size")
    verify_signature(root, path, sig)
    release_formats.validate(path, platform, suffix, version)
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    signature = sig.read_text(encoding="utf-8").strip()
    require(0 < len(signature) <= 2048, "invalid signature length")
    return {"url": f"{ORIGIN}/{tag}/{name}", "signature": signature, "sha256": digest, "size": size}


def metadata(root, directory, version, tag, notes, date):
    version_tag(version, tag)
    require(len(notes.encode()) <= 4096, "release notes too long")
    require(
        re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", date), "expected UTC publication date"
    )
    datetime.fromisoformat(date.replace("Z", "+00:00"))
    return {
        "schemaVersion": 1,
        "version": version,
        "tag": tag,
        "notes": notes,
        "pub_date": date,
        "platforms": {
            key: descriptor(root, directory, version, tag, key, suffix)
            for key, (_, suffix) in PLATFORMS.items()
        },
        "manualPackages": {
            key: descriptor(root, directory, version, tag, platform, suffix)
            for key, (platform, _, suffix) in MANUAL.items()
        },
    }


def verify_release(root, directory, version, tag):
    version_tag(version, tag)
    names = expected_names(version)
    expected = names | {name + ".sig" for name in names} | {"latest.json", "latest.json.sig"}
    require(
        {path.name for path in directory.iterdir()} == expected,
        "release asset set is incomplete or unexpected",
    )
    path = directory / "latest.json"
    require(path.stat().st_size <= 32768, "metadata exceeds limit")
    require(
        not path.is_symlink() and not (directory / "latest.json.sig").is_symlink(),
        "unsafe metadata",
    )
    verify_signature(
        root, path, directory / "latest.json.sig"
    )  # Verify exact bytes BEFORE parsing.
    actual = read_json(path)
    require(
        isinstance(actual, dict)
        and set(actual)
        == {"schemaVersion", "version", "tag", "notes", "pub_date", "platforms", "manualPackages"},
        "unexpected metadata fields",
    )
    require(
        type(actual["schemaVersion"]) is int and actual["schemaVersion"] == 1,
        "unsupported metadata schema",
    )
    require(actual["version"] == version and actual["tag"] == tag, "signed version/tag mismatch")
    require(
        isinstance(actual["notes"], str) and isinstance(actual["pub_date"], str),
        "invalid metadata text",
    )
    for section, keys in (("platforms", PLATFORMS), ("manualPackages", MANUAL)):
        require(
            isinstance(actual[section], dict) and set(actual[section]) == set(keys),
            "unexpected platform/package set",
        )
        for item in actual[section].values():
            require(isinstance(item, dict) and set(item) == FIELDS, "unexpected descriptor fields")
            require(
                type(item["size"]) is int and 0 < item["size"] <= MAX_SIZE,
                "invalid descriptor size",
            )
            require(
                all(isinstance(item[name], str) for name in ("url", "signature", "sha256")),
                "invalid descriptor text",
            )
    expected_metadata = metadata(root, directory, version, tag, actual["notes"], actual["pub_date"])
    require(
        actual == expected_metadata,
        "metadata does not match exact platforms, URLs, hashes, sizes or signatures",
    )
    require(path.read_bytes() == canonical(expected_metadata), "metadata is not canonical")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("preflight")
    check.add_argument("--version", required=True)
    check.add_argument("--tag", required=True)
    check.add_argument("--check-git", action="store_true")
    check.add_argument("--signing", action="store_true")
    check.add_argument("--config", type=Path)
    ci = sub.add_parser("ci-key")
    ci.add_argument("--directory", required=True, type=Path)
    gather = sub.add_parser("collect")
    gather.add_argument("--bundle", required=True, type=Path)
    gather.add_argument("--directory", required=True, type=Path)
    gather.add_argument("--platform", required=True, choices=PLATFORMS)
    gather.add_argument("--version", required=True)
    for name in ("metadata", "verify"):
        item = sub.add_parser(name)
        item.add_argument("--directory", required=True, type=Path)
        item.add_argument("--version", required=True)
        item.add_argument("--tag", required=True)
        if name == "metadata":
            item.add_argument("--date", required=True)
            item.add_argument("--notes", default="")
    args = parser.parse_args()
    root = args.root.resolve()
    if args.command == "preflight":
        preflight(root, args.version, args.tag, args.check_git, args.signing)
        if args.config:
            write_build_config(args.config)
    elif args.command == "ci-key":
        ci_key(root, args.directory)
    elif args.command == "collect":
        collect(root, args.bundle, args.directory, args.platform, args.version)
    elif args.command == "metadata":
        require(os.environ.get("SHACRAFT_UPDATER_TEST_BUILD") != "1", "CI artifacts cannot release")
        data = metadata(root, args.directory, args.version, args.tag, args.notes, args.date)
        path = args.directory / "latest.json"
        require(
            not path.exists() and not (args.directory / "latest.json.sig").exists(),
            "metadata already exists",
        )
        path.write_bytes(canonical(data))
        signer(root, path)
        verify_release(root, args.directory, args.version, args.tag)
    else:
        verify_release(root, args.directory, args.version, args.tag)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, TypeError) as error:
        print(f"Release validation failed: {error}", file=sys.stderr)
        sys.exit(1)
