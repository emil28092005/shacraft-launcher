"""Read-only package checks. Never execute or install a package under inspection."""

import io
import json
import plistlib
import shutil
import struct
import subprocess
import tarfile
from pathlib import Path, PurePosixPath


def require(condition, message):
    if not condition:
        raise ValueError(f"Package validation failed: {message}")


def pe_machine(data):
    require(len(data) >= 64 and data[:2] == b"MZ", "invalid PE DOS header")
    offset = struct.unpack_from("<I", data, 60)[0]
    require(
        64 <= offset <= len(data) - 26 and data[offset : offset + 4] == b"PE\0\0",
        "invalid PE header",
    )
    return struct.unpack_from("<H", data, offset + 4)[0], struct.unpack_from(
        "<H", data, offset + 24
    )[0]


def pe_x64(data):
    require(pe_machine(data) == (0x8664, 0x20B), "launcher executable must be AMD64 PE32+")


def appimage(path):
    with path.open("rb") as stream:
        data = stream.read(64)
    require(
        len(data) == 64 and data[:6] == b"\x7fELF\x02\x01", "AppImage must be little-endian ELF64"
    )
    require(data[8:11] == b"AI\x02", "AppImage must use Type 2 format")
    require(struct.unpack_from("<H", data, 18)[0] == 62, "AppImage must target AMD64")


def safe_member(name):
    name = name.removeprefix("./")
    path = PurePosixPath(name)
    require(
        not path.is_absolute() and ".." not in path.parts and "\\" not in name,
        "unsafe archive member",
    )
    return name


def mac_app(path, platform, version):
    with tarfile.open(path, "r:gz") as archive:
        members = {}
        for index, item in enumerate(archive):
            require(index < 10000, "too many app archive entries")
            name = safe_member(item.name).rstrip("/")
            require(name not in members, "duplicate app archive member")
            members[name] = item
        plists = [name for name in members if name.endswith(".app/Contents/Info.plist")]
        require(len(plists) == 1, "expected exactly one app Info.plist")
        info = members[plists[0]]
        require(info.isfile() and 0 < info.size <= 1024 * 1024, "invalid app Info.plist")
        details = plistlib.loads(archive.extractfile(info).read())
        executable = details.get("CFBundleExecutable", "")
        require(
            isinstance(executable, str)
            and executable not in {"", ".", ".."}
            and "/" not in executable
            and "\\" not in executable,
            "invalid app executable name",
        )
        require(details.get("CFBundleShortVersionString") == version, "app version mismatch")
        main = plists[0].removesuffix("Info.plist") + "MacOS/" + executable
        require(main in members and members[main].isfile(), "app executable missing or linked")
        data = archive.extractfile(members[main]).read(32)
        require(
            len(data) == 32 and data[:4] == b"\xcf\xfa\xed\xfe",
            "expected a thin little-endian Mach-O64 executable",
        )
        cpu, _, kind = struct.unpack_from("<III", data, 4)
        wanted = 0x0100000C if platform == "darwin-aarch64" else 0x01000007
        require(cpu == wanted and kind == 2, "app Mach-O architecture/type mismatch")


def deb(path, version):
    control = None
    debian_binary = None
    with path.open("rb") as stream:
        require(stream.read(8) == b"!<arch>\n", "invalid deb ar header")
        seen = set()
        while header := stream.read(60):
            require(len(header) == 60 and header[58:] == b"`\n", "invalid deb ar member")
            name = header[:16].decode("ascii").strip().removesuffix("/")
            require(name not in seen and len(seen) < 20, "duplicate/excessive deb members")
            seen.add(name)
            size = int(header[48:58].decode("ascii").strip())
            require(
                0 <= size <= 1024**3 and stream.tell() + size <= path.stat().st_size,
                "invalid/truncated deb member size",
            )
            if name == "debian-binary":
                require(size <= 16, "invalid debian-binary size")
                debian_binary = stream.read(size)
            elif name in {"control.tar.gz", "control.tar.xz", "control.tar"}:
                require(control is None and size <= 8 * 1024**2, "invalid deb control archive")
                control = stream.read(size)
                require(len(control) == size, "truncated deb control archive")
            else:
                stream.seek(size, 1)
            if size % 2:
                require(stream.read(1) == b"\n", "invalid ar padding")
    require(
        debian_binary == b"2.0\n"
        and control is not None
        and any(name.startswith("data.tar") for name in seen),
        "missing deb version/control/data",
    )
    with tarfile.open(fileobj=io.BytesIO(control), mode="r:*") as archive:
        matches = [item for item in archive if safe_member(item.name) == "control"]
        require(
            len(matches) == 1 and matches[0].isfile() and matches[0].size <= 1024**2,
            "invalid deb control file",
        )
        text = archive.extractfile(matches[0]).read().decode("utf-8")
    fields = {}
    for line in text.splitlines():
        if line and not line[0].isspace():
            key, separator, value = line.partition(":")
            require(separator and key not in fields, "invalid/duplicate deb control field")
            fields[key] = value.strip()
    require(fields.get("Architecture") == "amd64", "deb Architecture must be amd64")
    require(fields.get("Version") == version, "deb version mismatch")


def expected_nsis_payload(original):
    # tauri-cli-v2.11.4 / tauri-bundler::patch_binary changes only the first
    # complete token, then restores the original on-disk main after each bundle.
    # Reproduce that exact operation; never mask PE checksums, sections,
    # Authenticode certificate tables or arbitrary matching regions.
    token = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
    patched = b"__TAURI_BUNDLE_TYPE_VAR_NSS"
    offset = original.find(token)
    require(offset >= 0, "built launcher lacks the expected Tauri bundle-type marker")
    return original[:offset] + patched + original[offset + len(token) :]


def nsis_payload(path, built_main):
    tool = shutil.which("7z") or r"C:\Program Files\7-Zip\7z.exe"
    require(Path(tool).is_file(), "7-Zip is required to inspect NSIS payload")
    listing = subprocess.run(
        [tool, "l", "-slt", "-sccUTF-8", "--", str(path)],
        capture_output=True,
        check=False,
        timeout=60,
    )
    require(listing.returncode == 0, "7-Zip cannot inspect NSIS payload")
    matches = []
    for line in listing.stdout.decode("utf-8", errors="strict").splitlines():
        if line.startswith("Path = "):
            entry = line.removeprefix("Path = ")
            if PurePosixPath(entry.replace("\\", "/")).name == built_main.name:
                matches.append(entry)
    require(len(matches) == 1, "expected exactly one bundled launcher EXE in NSIS")
    extracted = subprocess.run(
        [tool, "x", "-so", "-bd", "--", str(path), matches[0]],
        capture_output=True,
        check=False,
        timeout=60,
    )
    require(extracted.returncode == 0, "7-Zip cannot extract NSIS launcher for inspection")
    pe_x64(extracted.stdout)
    require(
        extracted.stdout == expected_nsis_payload(built_main.read_bytes()),
        "NSIS payload differs from the x64 build plus the exact Tauri NSIS marker patch",
    )


def windows(path, suffix, version, built_main):
    with path.open("rb") as stream:
        header = stream.read(4096)
    if suffix == ".msi":
        require(
            len(header) >= 512
            and header[:8] == bytes.fromhex("d0cf11e0a1b11ae1")
            and header[28:30] == b"\xfe\xff",
            "invalid MSI compound-file header",
        )
    else:
        # Tauri's NSIS x64 package legitimately uses an x86-unicode installer stub.
        require(
            pe_machine(header) in {(0x14C, 0x10B), (0x8664, 0x20B)}, "unsupported NSIS PE wrapper"
        )
    if built_main is None:
        return  # Full payload/COM checks run on the Windows collection runner.
    require(built_main.is_file() and not built_main.is_symlink(), "built launcher EXE is missing")
    pe_x64(built_main.read_bytes())
    if suffix != ".msi":
        nsis_payload(path, built_main)
    else:
        script = Path(__file__).with_name("release_msi.ps1")
        result = subprocess.run(
            [
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-File",
                str(script),
                "-PackagePath",
                str(path.resolve()),
            ],
            capture_output=True,
            check=False,
            timeout=60,
        )
        require(result.returncode == 0, "cannot read MSI summary/properties")
        details = json.loads(result.stdout.decode("utf-8-sig"))
        require(details.get("template", "").split(";")[0] == "x64", "MSI template must be x64")
        require(details.get("version") == version, "MSI ProductVersion mismatch")


def validate(path, platform, suffix, version, built_main=None):
    if suffix == ".AppImage":
        appimage(path)
    elif suffix == ".deb":
        deb(path, version)
    elif suffix == ".app.tar.gz":
        mac_app(path, platform, version)
    elif suffix == ".dmg":
        with path.open("rb") as stream:
            require(path.stat().st_size >= 512, "truncated DMG")
            stream.seek(-512, 2)
            require(stream.read(4) == b"koly", "invalid DMG UDIF trailer")
    else:
        windows(path, suffix, version, built_main)
