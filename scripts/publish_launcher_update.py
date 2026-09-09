#!/usr/bin/env python3
"""Prepare and atomically publish a signed ShaCraft stable updater feed.

Requires Python 3.10+ and the minisign CLI. Only public keys are inputs.
Signing is deliberately a separate, operator-controlled action.
"""

import argparse
import base64
import binascii
import contextlib
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

ORIGIN = "https://shacraft.ru/downloads/shacraft-launcher/"
PLATFORMS = {
    "linux-x86_64": (".AppImage",),
    "windows-x86_64": (".exe", ".msi"),
    "darwin-x86_64": (".app.tar.gz",),
    "darwin-aarch64": (".app.tar.gz",),
}
FIELDS = {"version", "notes", "pub_date", "platforms"}
MAX_ARTIFACT_BYTES = 256 * 1024 * 1024
MAX_METADATA_BYTES = 64 * 1024


class InvalidRelease(ValueError):
    pass


def version_tuple(version):
    if not isinstance(version, str) or not re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version
    ):
        raise InvalidRelease("stable version must be plain MAJOR.MINOR.PATCH")
    parts = tuple(map(int, version.split(".")))
    if any(part > 2**64 - 1 for part in parts):
        raise InvalidRelease("version component exceeds SemVer range")
    return parts


def artifact_name(platform, filename):
    if platform not in PLATFORMS:
        raise InvalidRelease("unsupported updater platform")
    if not isinstance(filename, str) or not re.fullmatch(
        r"[A-Za-z0-9][A-Za-z0-9._-]{0,199}", filename
    ):
        raise InvalidRelease("artifact filename must be a plain ASCII filename")
    if not filename.endswith(PLATFORMS[platform]):
        raise InvalidRelease("artifact suffix does not match updater platform")
    return filename


def regular_file(path, limit):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size == 0 or info.st_size > limit:
        raise InvalidRelease("input must be a nonempty regular file within size limit")
    return info


def read_file(path, limit=MAX_METADATA_BYTES):
    regular_file(path, limit)
    with path.open("rb") as stream:
        value = stream.read(limit + 1)
    if len(value) > limit:
        raise InvalidRelease("input exceeds size limit")
    return value


def decode_tauri(value):
    if not isinstance(value, str) or not value or len(value) > MAX_METADATA_BYTES:
        raise InvalidRelease("invalid Tauri base64 value")
    try:
        decoded = base64.b64decode(value, validate=True)
        decoded.decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as exc:
        raise InvalidRelease("invalid Tauri base64 encoding") from exc
    if base64.b64encode(decoded).decode("ascii") != value:
        raise InvalidRelease("noncanonical Tauri base64 encoding")
    return decoded


def verify_signature(artifact, signature, public_key, minisign):
    # Tauri wraps the entire standard minisign text file in base64.
    signature_bytes = decode_tauri(signature)
    key_bytes = decode_tauri(public_key)
    with tempfile.TemporaryDirectory(prefix="shacraft-update-verify-") as temporary:
        root = Path(temporary)
        signature_path = root / "signature.minisig"
        key_path = root / "public.minisign.pub"
        signature_path.write_bytes(signature_bytes)
        key_path.write_bytes(key_bytes)
        try:
            result = subprocess.run(
                [minisign, "-V", "-q", "-m", str(artifact), "-x", str(signature_path),
                 "-p", str(key_path)],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, timeout=120, check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise InvalidRelease("minisign verification could not run") from exc
        if result.returncode != 0:
            raise InvalidRelease("signature verification failed")


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise InvalidRelease("duplicate JSON key")
            result[key] = value
        return result

    try:
        return json.loads(data, object_pairs_hook=unique)
    except (ValueError, UnicodeDecodeError) as exc:
        raise InvalidRelease("invalid release JSON") from exc


def canonical(payload):
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def verify_payload_bytes(payload_bytes, signature, public_key, minisign):
    with tempfile.TemporaryDirectory(prefix="shacraft-update-payload-") as temporary:
        immutable_payload = Path(temporary) / "payload.json"
        immutable_payload.write_bytes(payload_bytes)
        verify_signature(immutable_payload, signature, public_key, minisign)


def verified_previous(data, public_key, minisign):
    envelope = strict_json(data)
    if not isinstance(envelope, dict) or set(envelope) != FIELDS | {"signedPayload", "metadataSignature"}:
        raise InvalidRelease("existing feed must have authenticated metadata")
    payload_bytes = decode_tauri(envelope["signedPayload"])
    verify_payload_bytes(payload_bytes, envelope["metadataSignature"], public_key, minisign)
    payload = strict_json(payload_bytes)
    if payload != {key: envelope[key] for key in FIELDS}:
        raise InvalidRelease("existing feed fields differ from signed metadata")
    return payload


def validate_payload(payload, downloads_root, public_key, minisign):
    if not isinstance(payload, dict) or set(payload) != FIELDS:
        raise InvalidRelease("payload must contain exactly the four Tauri release fields")
    version_tuple(payload["version"])
    if not isinstance(payload["notes"], str) or len(payload["notes"]) > 8000:
        raise InvalidRelease("release notes must contain at most 8000 characters")
    if not isinstance(payload["pub_date"], str):
        raise InvalidRelease("release date must be RFC3339 UTC")
    try:
        datetime.strptime(payload["pub_date"], "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as exc:
        raise InvalidRelease("release date must be RFC3339 UTC") from exc
    platforms = payload["platforms"]
    if not isinstance(platforms, dict) or not platforms:
        raise InvalidRelease("at least one signed updater artifact is required")
    release_dir = downloads_root.resolve() / payload["version"]
    if release_dir.is_symlink() or not release_dir.is_dir():
        raise InvalidRelease("release directory must be an existing real directory")
    prefix = ORIGIN + payload["version"] + "/"
    for platform, artifact in platforms.items():
        if not isinstance(artifact, dict) or set(artifact) != {"url", "signature"}:
            raise InvalidRelease("artifact requires exactly url and signature")
        url = artifact["url"]
        if not isinstance(url, str) or not url.startswith(prefix):
            raise InvalidRelease("artifact must use the fixed ShaCraft release URL")
        filename = artifact_name(platform, url[len(prefix):])
        local_path = release_dir / filename
        before = regular_file(local_path, MAX_ARTIFACT_BYTES)
        verify_signature(local_path, artifact["signature"], public_key, minisign)
        after = regular_file(local_path, MAX_ARTIFACT_BYTES)
        if (before.st_ino, before.st_size, before.st_mtime_ns) != (
            after.st_ino, after.st_size, after.st_mtime_ns
        ):
            raise InvalidRelease("artifact changed during verification")


def atomic_write(destination, data):
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix="." + destination.name + ".", dir=destination.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            os.fchmod(stream.fileno(), 0o644)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
        directory = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary)


def prepare(args, public_key):
    version_tuple(args.version)
    artifacts = {}
    for item in args.artifact:
        platform, separator, filename = item.partition("=")
        if not separator or platform in artifacts:
            raise InvalidRelease("use each --artifact PLATFORM=FILENAME exactly once")
        artifact_name(platform, filename)
        signature = read_file(args.downloads_root / args.version / (filename + ".sig"), 16384).decode("ascii").strip()
        artifacts[platform] = {
            "url": ORIGIN + args.version + "/" + filename,
            "signature": signature,
        }
    payload = {
        "version": args.version,
        "notes": read_file(args.notes_file).decode("utf-8").strip(),
        "pub_date": args.pub_date or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": artifacts,
    }
    validate_payload(payload, args.downloads_root, public_key, args.minisign)
    atomic_write(args.payload, canonical(payload))
    return payload


def publish(args, public_key):
    payload_bytes = read_file(args.payload)
    payload = strict_json(payload_bytes)
    if not isinstance(payload, dict) or set(payload) != FIELDS:
        raise InvalidRelease("payload must contain exactly the four Tauri release fields")
    signature = read_file(args.signature, 16384).decode("ascii").strip()
    if canonical(payload) != payload_bytes:
        raise InvalidRelease("payload must be the exact canonical file from prepare")
    # Verify the captured bytes, so a changing operator input cannot replace
    # a verified file with different bytes in the feed.
    verify_payload_bytes(payload_bytes, signature, public_key, args.minisign)
    envelope = dict(payload)
    envelope["signedPayload"] = base64.b64encode(payload_bytes).decode("ascii")
    envelope["metadataSignature"] = signature
    data = json.dumps(envelope, ensure_ascii=False, indent=2).encode("utf-8") + b"\n"
    if len(data) > MAX_METADATA_BYTES:
        raise InvalidRelease("signed metadata exceeds size limit")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    lock = args.output.with_name("." + args.output.name + ".lock")
    descriptor = os.open(lock, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as lock_stream:
        fcntl.flock(lock_stream, fcntl.LOCK_EX)
        if args.output.exists() or args.output.is_symlink():
            previous = verified_previous(read_file(args.output), public_key, args.minisign)
            if version_tuple(payload["version"]) <= version_tuple(previous["version"]):
                raise InvalidRelease("stable publication must strictly increase version")
        validate_payload(payload, args.downloads_root, public_key, args.minisign)
        if not args.dry_run:
            atomic_write(args.output, data)
    return payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--downloads-root", type=Path, required=True)
    common.add_argument("--public-key", type=Path, required=True, help="Tauri outer-base64 .pub file")
    common.add_argument("--minisign", default="minisign")
    common.add_argument("--payload", type=Path, required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare", parents=[common])
    prepare_parser.add_argument("--version", required=True)
    prepare_parser.add_argument("--artifact", action="append", required=True, metavar="PLATFORM=FILENAME")
    prepare_parser.add_argument("--notes-file", type=Path, required=True)
    prepare_parser.add_argument("--pub-date")
    publish_parser = commands.add_parser("publish", parents=[common])
    publish_parser.add_argument("--signature", type=Path, required=True)
    publish_parser.add_argument("--output", type=Path, required=True)
    publish_parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        public_key = read_file(args.public_key, 16384).decode("ascii").strip()
        payload = prepare(args, public_key) if args.command == "prepare" else publish(args, public_key)
    except (InvalidRelease, OSError, UnicodeError) as exc:
        parser.exit(1, f"Release rejected: {exc}\n")
    print(f"{args.command}: {payload['version']} ({', '.join(sorted(payload['platforms']))})")


if __name__ == "__main__":
    main()
