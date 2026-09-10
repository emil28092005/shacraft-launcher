"""Publisher policy and real minisign verification; keys exist only in tempdirs."""

import argparse
import base64
import copy
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import publish_launcher_update as publisher

MINISIGN = os.environ.get("SHACRAFT_TEST_MINISIGN", "minisign")


def appimage_fixture():
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    header[8:11] = b"AI\x02"
    header[18:20] = b"\x3e\x00"
    return bytes(header) + b"isolated format fixture; not a runnable launcher"


class PolicyTests(unittest.TestCase):
    def test_stable_versions_are_strict_and_order_numerically(self):
        self.assertGreater(publisher.version_tuple("0.1.10"), publisher.version_tuple("0.1.9"))
        for value in ("v0.1.3", "0.01.3", "0.1.3-beta", "0.1.3+build", "../0.1.3", 3):
            with self.subTest(value=value), self.assertRaises(publisher.InvalidRelease):
                publisher.version_tuple(value)

    def test_platform_filename_policy(self):
        publisher.artifact_name("linux-x86_64", "ShaCraft.Launcher_0.1.3_amd64.AppImage")
        publisher.artifact_name("linux-x86_64-appimage", "ShaCraft.Launcher_0.1.4_amd64.AppImage")
        publisher.artifact_name("linux-x86_64-deb", "ShaCraft.Launcher_0.1.4_amd64.deb")
        for platform, filename in (
            ("linux-x86_64", "../bad.AppImage"), ("linux-x86_64", "foo.AppImage?secret"),
            ("linux-x86_64", "%2e%2e.AppImage"), ("linux-x86_64", "install.exe"),
            ("unknown", "test.AppImage"), ("darwin-aarch64", "installer.dmg"),
            ("linux-x86_64", "install.deb"), ("linux-x86_64-appimage", "install.deb"),
            ("linux-x86_64-deb", "install.AppImage"),
        ):
            with self.subTest(filename=filename), self.assertRaises(publisher.InvalidRelease):
                publisher.artifact_name(platform, filename)

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaises(publisher.InvalidRelease):
            publisher.strict_json(b'{"version":"0.1.3","version":"9.0.0"}')

    def test_package_inspection_is_bounded_and_clears_environment(self):
        with self.assertRaisesRegex(publisher.InvalidRelease, "output limit"):
            publisher.bounded_command_output([sys.executable, "-c", "print('x' * 8192)"], limit=128)
        with self.assertRaisesRegex(publisher.InvalidRelease, "timed out"):
            publisher.bounded_command_output([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.1)
        os.environ["SHACRAFT_INSPECTION_SECRET_TEST"] = "must-not-be-inherited"
        try:
            output = publisher.bounded_command_output([
                sys.executable, "-c", "import os; print(os.getenv('SHACRAFT_INSPECTION_SECRET_TEST', 'clean'))",
            ])
        finally:
            del os.environ["SHACRAFT_INSPECTION_SECRET_TEST"]
        self.assertEqual(output, b"clean\n")


@unittest.skipUnless(shutil.which(MINISIGN), "minisign CLI required for signature integration tests")
class SignatureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="shacraft-update-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.key = self.root / "fixture.key"
        public = self.root / "fixture.pub"
        subprocess.run(
            [MINISIGN, "-G", "-W", "-p", str(public), "-s", str(self.key)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True,
        )
        self.public_key = base64.b64encode(public.read_bytes()).decode("ascii")
        self.downloads = self.root / "downloads"
        release = self.downloads / "0.1.3"
        release.mkdir(parents=True)
        self.artifact = release / "fixture.AppImage"
        self.artifact.write_bytes(appimage_fixture())
        self.payload = {
            "version": "0.1.3", "notes": "Проверка обновления", "pub_date": "2026-09-10T00:00:00Z",
            "platforms": {"linux-x86_64": {
                "url": publisher.ORIGIN + "0.1.3/fixture.AppImage",
                "signature": self.sign(self.artifact),
            }},
        }
        self.payload_path = self.root / "payload.json"
        self.output = self.root / "stable.json"

    def sign(self, path):
        signature_path = path.with_name(path.name + ".minisig")
        subprocess.run(
            [MINISIGN, "-S", "-s", str(self.key), "-m", str(path), "-x", str(signature_path), "-q"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True,
        )
        signature = base64.b64encode(signature_path.read_bytes()).decode("ascii")
        path.with_name(path.name + ".sig").write_text(signature, encoding="ascii")
        return signature

    def publish(self, payload=None, dry_run=False):
        self.payload_path.write_bytes(publisher.canonical(payload or self.payload))
        self.sign(self.payload_path)
        args = argparse.Namespace(
            payload=self.payload_path, signature=self.payload_path.with_name("payload.json.sig"),
            output=self.output, downloads_root=self.downloads, minisign=MINISIGN, dry_run=dry_run,
        )
        return publisher.publish(args, self.public_key)

    def test_valid_signed_feed_binds_metadata_and_artifact(self):
        self.publish()
        envelope = publisher.strict_json(self.output.read_bytes())
        payload_bytes = publisher.decode_tauri(envelope["signedPayload"])
        self.assertEqual(publisher.strict_json(payload_bytes), self.payload)
        self.assertEqual({key: envelope[key] for key in publisher.FIELDS}, self.payload)
        self.assertIn("metadataSignature", envelope)
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o644)

    def test_tampered_artifact_is_rejected_before_publication(self):
        self.artifact.write_bytes(b"replaced executable")
        with self.assertRaisesRegex(publisher.InvalidRelease, "signature verification failed"):
            self.publish()
        self.assertFalse(self.output.exists())

    def test_tampered_metadata_signature_is_rejected(self):
        self.payload_path.write_bytes(publisher.canonical(self.payload))
        signature = self.sign(self.payload_path)
        self.payload["notes"] = "Changed after signing"
        self.payload_path.write_bytes(publisher.canonical(self.payload))
        with self.assertRaisesRegex(publisher.InvalidRelease, "signature verification failed"):
            publisher.verify_signature(self.payload_path, signature, self.public_key, MINISIGN)

    def test_same_version_or_downgrade_keeps_original_feed(self):
        self.publish()
        original = self.output.read_bytes()
        for version in ("0.1.3", "0.1.2"):
            payload = copy.deepcopy(self.payload)
            payload["version"] = version
            with self.subTest(version=version), self.assertRaisesRegex(
                publisher.InvalidRelease, "strictly increase"
            ):
                self.publish(payload)
            self.assertEqual(self.output.read_bytes(), original)

    def test_foreign_url_cannot_be_signed_into_feed(self):
        self.payload["platforms"]["linux-x86_64"]["url"] = "https://example.com/test.AppImage"
        with self.assertRaisesRegex(publisher.InvalidRelease, "fixed ShaCraft release URL"):
            self.publish()
        self.assertFalse(self.output.exists())

    def test_previous_version_must_also_be_authenticated(self):
        self.publish()
        envelope = publisher.strict_json(self.output.read_bytes())
        envelope["version"] = "99.0.0"
        self.output.write_bytes(publisher.canonical(envelope))
        with self.assertRaisesRegex(publisher.InvalidRelease, "differ from signed metadata"):
            self.publish()

    def test_missing_signature_or_symlink_is_rejected(self):
        original = self.artifact.read_bytes()
        target = self.root / "outside.AppImage"
        target.write_bytes(original)
        self.artifact.unlink()
        self.artifact.symlink_to(target)
        with self.assertRaisesRegex(publisher.InvalidRelease, "regular file"):
            self.publish()
        self.artifact.unlink()
        self.artifact.write_bytes(original)
        self.payload["platforms"]["linux-x86_64"].pop("signature")
        with self.assertRaisesRegex(publisher.InvalidRelease, "exactly url and signature"):
            self.publish()

    def test_dry_run_verifies_without_creating_feed(self):
        self.publish(dry_run=True)
        self.assertFalse(self.output.exists())

    def make_deb(self, package="sha-craft-launcher", version=None, architecture="amd64"):
        if not Path(publisher.DPKG_DEB).is_file():
            self.skipTest("dpkg-deb required for real deb validation")
        version = version or self.payload["version"]
        tree = self.root / "deb-tree"
        control = tree / "DEBIAN"
        control.mkdir(parents=True, exist_ok=True)
        (control / "control").write_text(
            f"Package: {package}\nVersion: {version}\nArchitecture: {architecture}\n"
            "Maintainer: Test <test@example.invalid>\nDescription: isolated updater fixture\n",
            encoding="ascii",
        )
        # Inspection must not execute a package script, even for an authenticated package.
        script = control / "preinst"
        script.write_text(f"#!/bin/sh\ntouch '{self.root / 'script-executed'}'\n", encoding="ascii")
        script.chmod(0o755)
        deb = self.artifact.with_name("fixture.deb")
        subprocess.run(
            [publisher.DPKG_DEB, "--build", "--root-owner-group", str(tree), str(deb)],
            env=publisher.PACKAGE_TOOL_ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            timeout=10, check=True,
        )
        self.payload["platforms"]["linux-x86_64-appimage"] = copy.deepcopy(
            self.payload["platforms"]["linux-x86_64"]
        )
        self.payload["platforms"]["linux-x86_64-deb"] = {
            "url": publisher.ORIGIN + self.payload["version"] + "/fixture.deb", "signature": self.sign(deb),
        }
        return deb

    def test_format_aware_release_preserves_legacy_appimage_and_verifies_real_deb(self):
        self.make_deb()
        notes = self.root / "notes.txt"
        notes.write_text(self.payload["notes"], encoding="utf-8")
        args = argparse.Namespace(
            version="0.1.3", artifact=[
                "linux-x86_64=fixture.AppImage", "linux-x86_64-appimage=fixture.AppImage",
                "linux-x86_64-deb=fixture.deb",
            ], downloads_root=self.downloads, notes_file=notes, payload=self.payload_path,
            pub_date=self.payload["pub_date"], minisign=MINISIGN,
        )
        self.assertEqual(publisher.prepare(args, self.public_key), self.payload)
        self.publish()
        feed = publisher.verified_previous(self.output.read_bytes(), self.public_key, MINISIGN)
        self.assertEqual(feed["platforms"]["linux-x86_64"], feed["platforms"]["linux-x86_64-appimage"])
        self.assertEqual(set(feed["platforms"]), {"linux-x86_64", "linux-x86_64-appimage", "linux-x86_64-deb"})
        self.assertFalse((self.root / "script-executed").exists())

    def test_authenticated_legacy_feed_advances_to_format_aware_release(self):
        self.publish()
        old_release = self.artifact.parent
        old_bytes = self.artifact.read_bytes()
        new_release = self.downloads / "0.1.4"
        shutil.copytree(old_release, new_release)
        self.artifact = new_release / self.artifact.name
        self.payload["version"] = "0.1.4"
        self.payload["platforms"]["linux-x86_64"]["url"] = publisher.ORIGIN + "0.1.4/fixture.AppImage"
        self.make_deb()
        self.publish()
        feed = publisher.verified_previous(self.output.read_bytes(), self.public_key, MINISIGN)
        self.assertEqual(feed["version"], "0.1.4")
        self.assertEqual(len(feed["platforms"]), 3)
        self.assertEqual((old_release / self.artifact.name).read_bytes(), old_bytes)

    def test_linux_format_release_cannot_drop_or_repoint_legacy_entry(self):
        self.make_deb()
        for key in ("linux-x86_64", "linux-x86_64-appimage"):
            payload = copy.deepcopy(self.payload)
            del payload["platforms"][key]
            with self.subTest(key=key), self.assertRaisesRegex(publisher.InvalidRelease, "identical legacy"):
                self.publish(payload)
        payload = copy.deepcopy(self.payload)
        payload["platforms"]["linux-x86_64-appimage"]["url"] = publisher.ORIGIN + "0.1.3/other.AppImage"
        with self.assertRaisesRegex(publisher.InvalidRelease, "identical legacy"):
            self.publish(payload)
        self.assertFalse(self.output.exists())

    def test_deb_identity_must_match_application_signed_version_and_architecture(self):
        for changes in ({"package": "another-launcher"}, {"version": "9.0.0"}, {"architecture": "arm64"}):
            with self.subTest(changes=changes):
                self.make_deb(**changes)
                with self.assertRaisesRegex(publisher.InvalidRelease, "deb identity"):
                    self.publish()
                self.assertFalse(self.output.exists())

    def test_signed_invalid_deb_is_rejected_without_running_package_scripts(self):
        deb = self.make_deb()
        deb.write_bytes(b"not a Debian archive")
        self.payload["platforms"]["linux-x86_64-deb"]["signature"] = self.sign(deb)
        with self.assertRaisesRegex(publisher.InvalidRelease, "package inspection failed"):
            self.publish()
        self.assertFalse((self.root / "script-executed").exists())
        self.assertFalse(self.output.exists())

    def test_signed_wrong_appimage_format_is_rejected(self):
        for changed_slice, replacement in ((slice(8, 11), b"AI\x01"), (slice(18, 20), b"\xb7\x00")):
            malformed = bytearray(appimage_fixture())
            malformed[changed_slice] = replacement
            self.artifact.write_bytes(malformed)
            self.payload["platforms"]["linux-x86_64"]["signature"] = self.sign(self.artifact)
            with self.subTest(replacement=replacement), self.assertRaisesRegex(publisher.InvalidRelease, "type-2 x86_64"):
                self.publish()
            self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
