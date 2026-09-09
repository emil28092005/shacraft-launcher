"""Real Tauri signature checks using disposable keys, never release credentials."""

import io
import json
import os
import plistlib
import shutil
import struct
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release
from release_assets_test import DraftAssetGateTests  # noqa: F401; unittest discovery
from release_gate_test import ReleaseGateTests  # noqa: F401; unittest discovery
import release_formats
import release_github

ROOT = Path(os.environ.get("RELEASE_TEST_ROOT", Path(__file__).resolve().parents[1]))


def fixture_pe(machine=0x8664, magic=0x20B):
    data = bytearray(256)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, machine)
    struct.pack_into("<H", data, 152, magic)
    marker = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
    data[200 : 200 + len(marker)] = marker
    return bytes(data)


def fixture_tar(files):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as archive:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return stream.getvalue()


def fixture_deb(architecture="amd64", version="0.3.0"):
    control = fixture_tar(
        {
            "./control": f"Package: shacraft-launcher\nVersion: {version}\nArchitecture: {architecture}\n".encode()
        }
    )
    result = bytearray(b"!<arch>\n")
    for name, data in [
        ("debian-binary", b"2.0\n"),
        ("control.tar.gz", control),
        ("data.tar.gz", fixture_tar({})),
    ]:
        header = f"{name + '/':<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(data):<10}`\n".encode()
        assert len(header) == 60
        result.extend(header + data + (b"\n" if len(data) % 2 else b""))
    return bytes(result)


def fixture_package(name, version="0.3.0"):
    if name.endswith(".app.tar.gz"):
        cpu = 0x0100000C if "aarch64" in name else 0x01000007
        executable = struct.pack("<IIIIIIII", 0xFEEDFACF, cpu, 0, 2, 0, 0, 0, 0)
        return fixture_tar(
            {
                "ShaCraft.app/Contents/Info.plist": plistlib.dumps(
                    {
                        "CFBundleExecutable": "shacraft-launcher",
                        "CFBundleShortVersionString": version,
                    }
                ),
                "ShaCraft.app/Contents/MacOS/shacraft-launcher": executable,
            }
        )
    if name.endswith(".AppImage"):
        data = bytearray(64)
        data[:6] = b"\x7fELF\x02\x01"
        data[8:11] = b"AI\x02"
        struct.pack_into("<H", data, 18, 62)
        return bytes(data)
    if name.endswith(".deb"):
        return fixture_deb(version=version)
    if name.endswith(".dmg"):
        return b"koly" + bytes(508)
    if name.endswith(".msi"):
        data = bytearray(512)
        data[:8] = bytes.fromhex("d0cf11e0a1b11ae1")
        data[28:30] = b"\xfe\xff"
        return bytes(data)
    return fixture_pe(0x14C, 0x10B)  # x86 NSIS wrapper is valid for an x64 payload.


class ReleaseTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="shacraft-release-tests-")
        cls.base = Path(cls.temporary.name)
        # A new unencrypted disposable key per test process. No static private
        # key or credentials are stored in the repository or test output.
        cls.key = cls.base / "test-only.key"
        environment = dict(os.environ)
        for name in (
            "TAURI_SIGNING_PRIVATE_KEY",
            "TAURI_SIGNING_PRIVATE_KEY_PATH",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        ):
            environment.pop(name, None)
        result = subprocess.run(
            [
                "node",
                str(ROOT / "node_modules/@tauri-apps/cli/tauri.js"),
                "signer",
                "generate",
                "--ci",
                "--password",
                "",
                "--write-keys",
                str(cls.key),
            ],
            env=environment,
            capture_output=True,
            timeout=120,
            check=False,
        )
        if result.returncode:
            raise RuntimeError("disposable Tauri signer setup failed")
        cls.key.chmod(0o600)
        cls.key_values = {
            "SHACRAFT_UPDATER_PUBLIC_KEY": Path(str(cls.key) + ".pub").read_text().strip(),
            "TAURI_SIGNING_PRIVATE_KEY": cls.key.read_text().strip(),
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "",
            "SHACRAFT_UPDATER_TEST_BUILD": "0",
        }
        cls.originals = cls.base / "complete"
        cls.originals.mkdir()
        with patch.dict(os.environ, cls.key_values):
            for name in release.expected_names("0.3.0"):
                path = cls.originals / name
                path.write_bytes(fixture_package(name))
                release.signer(ROOT, path)
            data = release.metadata(
                ROOT, cls.originals, "0.3.0", "v0.3.0", "Test only", "2026-09-09T00:00:00Z"
            )
            (cls.originals / "latest.json").write_bytes(release.canonical(data))
            release.signer(ROOT, cls.originals / "latest.json")

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def setUp(self):
        self.case = tempfile.TemporaryDirectory(dir=self.base)
        self.directory = Path(self.case.name) / "assets"
        shutil.copytree(self.originals, self.directory)
        self.environment = patch.dict(os.environ, self.key_values)
        self.environment.start()

    def tearDown(self):
        self.environment.stop()
        self.case.cleanup()

    def verify(self):
        release.verify_release(ROOT, self.directory, "0.3.0", "v0.3.0")

    def resign_metadata(self, mutate):
        path = self.directory / "latest.json"
        data = json.loads(path.read_bytes())
        mutate(data)
        path.write_bytes(release.canonical(data))
        release.signer(ROOT, path)

    def test_complete_release_and_metadata_generation_are_deterministic(self):
        self.verify()
        first = release.metadata(
            ROOT, self.directory, "0.3.0", "v0.3.0", "Test only", "2026-09-09T00:00:00Z"
        )
        self.assertEqual(release.canonical(first), (self.directory / "latest.json").read_bytes())

    def test_package_bit_flip_fails_actual_plugin_verification(self):
        path = self.directory / release.filename("0.3.0", "linux-x86_64", ".AppImage")
        path.write_bytes(path.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "invalid updater signature"):
            self.verify()

    def test_metadata_substitution_is_rejected_before_json_parsing(self):
        (self.directory / "latest.json").write_bytes(b"not even JSON")
        with self.assertRaisesRegex(ValueError, "invalid updater signature"):
            self.verify()

    def test_valid_signature_for_other_artifact_does_not_suffice(self):
        linux = release.filename("0.3.0", "linux-x86_64", ".AppImage")
        windows = release.filename("0.3.0", "windows-x86_64", "-setup.exe")
        shutil.copyfile(self.directory / (windows + ".sig"), self.directory / (linux + ".sig"))
        with self.assertRaisesRegex(ValueError, "invalid updater signature"):
            self.verify()

    def test_wrong_public_key_fails_real_verifier(self):
        encoded = release.public_key()
        import base64

        lines = base64.b64decode(encoded).decode().splitlines()
        raw = bytearray(base64.b64decode(lines[1]))
        raw[-1] ^= 1
        lines[1] = base64.b64encode(raw).decode()
        wrong = base64.b64encode(("\n".join(lines) + "\n").encode()).decode()
        with patch.dict(os.environ, {"SHACRAFT_UPDATER_PUBLIC_KEY": wrong}):
            with self.assertRaisesRegex(ValueError, "invalid updater signature"):
                self.verify()

    def test_all_platforms_and_manual_packages_are_mandatory(self):
        for name in sorted(release.expected_names("0.3.0")):
            path = self.directory / (name + ".sig")
            original = path.read_bytes()
            path.unlink()
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "asset set"):
                self.verify()
            path.write_bytes(original)

    def test_extra_ci_marker_or_file_blocks_promotion(self):
        (self.directory / "CI_NOT_FOR_RELEASE.txt").write_text("test key")
        with self.assertRaisesRegex(ValueError, "asset set"):
            self.verify()

    def test_signed_metadata_must_match_tag_and_exact_source_assets(self):
        alterations = [
            lambda x: x.update(version="0.2.0"),
            lambda x: x.update(tag="v0.4.0"),
            lambda x: x["platforms"].pop("darwin-aarch64"),
            lambda x: x["platforms"].update({"linux-aarch64": x["platforms"]["linux-x86_64"]}),
            lambda x: x["manualPackages"].pop("windows-x86_64-msi"),
            lambda x: x["platforms"]["linux-x86_64"].update(url="https://evil.invalid/package"),
            lambda x: x["platforms"]["linux-x86_64"].update(
                url=x["platforms"]["linux-x86_64"]["url"].replace("v0.3.0", "v0.2.0")
            ),
            lambda x: x["platforms"]["linux-x86_64"].update(sha256="0" * 64),
            lambda x: x["platforms"]["linux-x86_64"].update(size=True),
            lambda x: x.update(pub_date="2026-09-09T00:00:00+03:00"),
            lambda x: x.update(extra="not allowed"),
        ]
        for index, alter in enumerate(alterations):
            shutil.copyfile(self.originals / "latest.json", self.directory / "latest.json")
            self.resign_metadata(alter)
            with self.subTest(index=index), self.assertRaises(ValueError):
                self.verify()

    def test_duplicate_json_fields_are_rejected_even_if_signed(self):
        path = self.directory / "latest.json"
        path.write_bytes(
            path.read_bytes().replace(
                b'"schemaVersion": 1,', b'"schemaVersion": 1, "schemaVersion": 1,'
            )
        )
        release.signer(ROOT, path)
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            self.verify()

    def test_collect_requires_generated_updater_sig_and_signs_manual_deb(self):
        bundle = Path(self.case.name) / "bundle"
        (bundle / "appimage").mkdir(parents=True)
        (bundle / "deb").mkdir()
        app = bundle / "appimage/Test.AppImage"
        app.write_bytes(fixture_package("Test.AppImage"))
        (bundle / "deb/Test.deb").write_bytes(fixture_package("Test.deb"))
        output = Path(self.case.name) / "collected"
        with self.assertRaisesRegex(ValueError, "missing generated updater signature"):
            release.collect(ROOT, bundle, output, "linux-x86_64", "0.3.0")
        shutil.rmtree(output)
        release.signer(ROOT, app)
        release.collect(ROOT, bundle, output, "linux-x86_64", "0.3.0")
        self.assertEqual(len(list(output.iterdir())), 4)
        for suffix in (".AppImage", ".deb"):
            data = output / release.filename("0.3.0", "linux-x86_64", suffix)
            release.verify_signature(ROOT, data, Path(str(data) + ".sig"))

    def test_missing_key_fails_and_optional_password_never_prompts(self):
        data = self.directory / "latest.json"
        with patch.dict(os.environ, {"TAURI_SIGNING_PRIVATE_KEY": ""}):
            with self.assertRaisesRegex(ValueError, "PRIVATE_KEY is missing"):
                release.signer(ROOT, data)
        with patch.dict(os.environ):
            os.environ.pop("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", None)
            release.signer(ROOT, data)
            release.verify_signature(ROOT, data, Path(str(data) + ".sig"))

    def test_release_preflight_rejects_placeholder_mismatch_and_test_mode(self):
        source = Path(self.case.name) / "source"
        (source / "src-tauri").mkdir(parents=True)
        (source / "package.json").write_text('{"version":"0.3.0"}')
        (source / "package-lock.json").write_text(
            '{"version":"0.3.0","packages":{"":{"version":"0.3.0"}}}'
        )
        (source / "src-tauri/tauri.conf.json").write_text('{"version":"0.3.0"}')
        (source / "src-tauri/Cargo.toml").write_text('[package]\nversion="0.3.0"\n')
        (source / "src-tauri/updater-public-key.txt").write_text("UNCONFIGURED")
        with self.assertRaisesRegex(ValueError, "differs from committed"):
            release.preflight(source, "0.3.0", "v0.3.0", signing=True)
        with patch.dict(os.environ, {"SHACRAFT_UPDATER_TEST_BUILD": "1"}):
            with self.assertRaisesRegex(ValueError, "test keys cannot release"):
                release.preflight(source, "0.3.0", "v0.3.0", signing=True)
        (source / "package.json").write_text('{"version":"0.3.1"}')
        with self.assertRaisesRegex(ValueError, "source versions disagree"):
            release.preflight(source, "0.3.0", "v0.3.0")


class PackageFormatTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="shacraft-format-tests-")
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def package(self, name, data=None):
        path = self.root / name
        path.write_bytes(fixture_package(name) if data is None else data)
        return path

    def test_mac_archive_binds_main_executable_cpu_and_version(self):
        path = self.package("darwin-aarch64.app.tar.gz")
        release_formats.mac_app(path, "darwin-aarch64", "0.3.0")
        for platform, version in [("darwin-x86_64", "0.3.0"), ("darwin-aarch64", "0.2.0")]:
            with self.subTest(platform=platform, version=version), self.assertRaises(ValueError):
                release_formats.mac_app(path, platform, version)

    def test_mac_archive_rejects_traversal_and_missing_binary(self):
        for files in (
            {"../Bad.app/Contents/Info.plist": b"bad"},
            {
                "ShaCraft.app/Contents/Info.plist": plistlib.dumps(
                    {"CFBundleExecutable": "missing", "CFBundleShortVersionString": "0.3.0"}
                )
            },
        ):
            path = self.package("darwin-aarch64.app.tar.gz", fixture_tar(files))
            with self.assertRaises(ValueError):
                release_formats.mac_app(path, "darwin-aarch64", "0.3.0")

    def test_appimage_rejects_arm64_wrong_class_and_type1(self):
        path = self.package("linux-x86_64.AppImage")
        release_formats.appimage(path)
        for offset, data in [(18, struct.pack("<H", 183)), (4, b"\x01"), (10, b"\x01")]:
            bad = bytearray(fixture_package(path.name))
            bad[offset : offset + len(data)] = data
            path.write_bytes(bad)
            with self.assertRaises(ValueError):
                release_formats.appimage(path)

    def test_deb_control_binds_architecture_and_version(self):
        path = self.package("linux-x86_64.deb")
        release_formats.deb(path, "0.3.0")
        for architecture, version in [("arm64", "0.3.0"), ("amd64", "0.2.0")]:
            path.write_bytes(fixture_deb(architecture, version))
            with self.assertRaises(ValueError):
                release_formats.deb(path, "0.3.0")

    def test_nsis_stub_may_be_x86_but_extracted_main_must_equal_x64_build(self):
        wrapper = self.package("setup.exe")
        main = self.package("shacraft-launcher.exe", fixture_pe())
        listing = subprocess.CompletedProcess(
            [], 0, b"Path = setup.exe\nPath = $INSTDIR/shacraft-launcher.exe\n", b""
        )
        for extracted, success in [
            (release_formats.expected_nsis_payload(fixture_pe()), True),
            (fixture_pe(), False),
            (release_formats.expected_nsis_payload(fixture_pe(0xAA64, 0x20B)), False),
            (release_formats.expected_nsis_payload(fixture_pe()) + b"different", False),
        ]:
            with (
                patch.object(release_formats.shutil, "which", return_value=str(main)),
                patch.object(
                    release_formats.subprocess,
                    "run",
                    side_effect=[listing, subprocess.CompletedProcess([], 0, extracted, b"")],
                ),
            ):
                if success:
                    release_formats.windows(wrapper, "-setup.exe", "0.3.0", main)
                else:
                    with self.assertRaises(ValueError):
                        release_formats.windows(wrapper, "-setup.exe", "0.3.0", main)

    def test_nsis_identity_allows_only_the_actual_first_bundle_marker_patch(self):
        original = fixture_pe() + b"__TAURI_BUNDLE_TYPE_VAR_UNK"
        expected = release_formats.expected_nsis_payload(original)
        self.assertEqual(
            expected,
            original.replace(b"__TAURI_BUNDLE_TYPE_VAR_UNK", b"__TAURI_BUNDLE_TYPE_VAR_NSS", 1),
        )
        self.assertEqual(expected.count(b"__TAURI_BUNDLE_TYPE_VAR_UNK"), 1)
        with self.assertRaisesRegex(ValueError, "bundle-type marker"):
            release_formats.expected_nsis_payload(
                fixture_pe().replace(b"__TAURI_BUNDLE_TYPE_VAR_UNK", b"__TAURI_BUNDLE_TYPE_VAR_MSI")
            )
        # No certificate table/checksum/other byte range is ignored.
        mutated = bytearray(expected)
        mutated[190] ^= 1
        self.assertNotEqual(bytes(mutated), release_formats.expected_nsis_payload(original))
        self.assertNotEqual(
            expected.replace(b"_VAR_NSS", b"_VAR_MSI"),
            release_formats.expected_nsis_payload(original),
        )

    def test_msi_readonly_metadata_requires_x64_and_version(self):
        msi = self.package("setup.msi")
        main = self.package("shacraft-launcher.exe", fixture_pe())
        for architecture, version, success in [
            ("x64;1033", "0.3.0", True),
            ("Intel;1033", "0.3.0", False),
            ("x64;1033", "0.2.0", False),
        ]:
            result = subprocess.CompletedProcess(
                [], 0, json.dumps({"template": architecture, "version": version}).encode(), b""
            )
            with patch.object(release_formats.subprocess, "run", return_value=result) as command:
                if success:
                    release_formats.windows(msi, ".msi", "0.3.0", main)
                else:
                    with self.assertRaises(ValueError):
                        release_formats.windows(msi, ".msi", "0.3.0", main)
                self.assertIn("release_msi.ps1", command.call_args.args[0][4])


class WorkflowPolicyTests(unittest.TestCase):
    def protected(self):
        return {
            "deployment_branch_policy": {
                "protected_branches": True,
                "custom_branch_policies": False,
            },
            "protection_rules": [
                {
                    "type": "required_reviewers",
                    "prevent_self_review": True,
                    "reviewers": [{"type": "User", "reviewer": {"id": 123}}],
                }
            ],
        }

    def test_protected_environment_requires_existing_independent_review(self):
        release_github.validate_environment(self.protected())
        for data in (
            None,
            {},
            {"deployment_branch_policy": {}},
            dict(self.protected(), protection_rules=[]),
        ):
            with self.subTest(data=data), self.assertRaises(ValueError):
                release_github.validate_environment(data)
        for mutate in (
            lambda d: d["deployment_branch_policy"].update(protected_branches=False),
            lambda d: d["protection_rules"][0].update(prevent_self_review=False),
            lambda d: d["protection_rules"][0].update(reviewers=[]),
        ):
            data = self.protected()
            mutate(data)
            with self.assertRaises(ValueError):
                release_github.validate_environment(data)

    def test_gate_does_not_emit_missing_environment_name(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            env = {
                "GITHUB_REPOSITORY": release.REPOSITORY,
                "GITHUB_EVENT_NAME": "workflow_dispatch",
                "GITHUB_REF": "refs/heads/main",
                "GITHUB_OUTPUT": str(output),
            }
            with (
                patch.dict(os.environ, env),
                patch.object(release_github, "api", side_effect=[{"protected": True}, None]),
            ):
                with self.assertRaisesRegex(ValueError, "absent"):
                    release_github.gate("launcher-release")
            self.assertFalse(output.exists())

    def test_publication_never_runs_without_exact_confirmation(self):
        env = {
            "GITHUB_REPOSITORY": release.REPOSITORY,
            "GITHUB_EVENT_NAME": "workflow_dispatch",
            "GITHUB_REF": "refs/heads/main",
        }
        with patch.dict(os.environ, env), patch.object(release_github, "gh") as client:
            with self.assertRaisesRegex(ValueError, "confirmation"):
                release_github.publish(ROOT, "0.3.0", "v0.3.0", "publish v0.2.0")
            client.assert_not_called()

    def test_asset_snapshot_rejects_public_or_incomplete_draft(self):
        data = {
            "draft": True,
            "prerelease": False,
            "assets": [
                {
                    "id": 1,
                    "name": "latest.json",
                    "size": 10,
                    "state": "uploaded",
                    "digest": "sha256:abc",
                }
            ],
        }
        self.assertEqual(release_github.asset_snapshot(data)["latest.json"][0], 1)
        for change in (
            {"draft": False},
            {"prerelease": True},
            {"assets": [dict(data["assets"][0], state="new")]},
            {"assets": data["assets"] * 2},
        ):
            with self.assertRaises(ValueError):
                release_github.asset_snapshot(dict(data, **change))

    def test_version_and_tag_cannot_inject_paths_or_exceed_msi_limits(self):
        release.version_tag("0.3.0", "v0.3.0")
        for version, tag in (
            ("0.3.0", "main"),
            ("0.3.0", "v0.3.0/evil"),
            ("01.3.0", "v01.3.0"),
            ("0.3.0-beta", "v0.3.0-beta"),
            ("256.0.0", "v256.0.0"),
            ("0.0.65536", "v0.0.65536"),
        ):
            with self.subTest(version=version, tag=tag), self.assertRaises(ValueError):
                release.version_tag(version, tag)


if __name__ == "__main__":
    unittest.main()
