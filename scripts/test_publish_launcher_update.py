"""Publisher policy and real minisign verification; keys exist only in tempdirs."""

import argparse
import base64
import copy
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import publish_launcher_update as publisher

MINISIGN = os.environ.get("SHACRAFT_TEST_MINISIGN", "minisign")


class PolicyTests(unittest.TestCase):
    def test_stable_versions_are_strict_and_order_numerically(self):
        self.assertGreater(publisher.version_tuple("0.1.10"), publisher.version_tuple("0.1.9"))
        for value in ("v0.1.3", "0.01.3", "0.1.3-beta", "0.1.3+build", "../0.1.3", 3):
            with self.subTest(value=value), self.assertRaises(publisher.InvalidRelease):
                publisher.version_tuple(value)

    def test_platform_filename_policy(self):
        publisher.artifact_name("linux-x86_64", "ShaCraft.Launcher_0.1.3_amd64.AppImage")
        for platform, filename in (
            ("linux-x86_64", "../bad.AppImage"), ("linux-x86_64", "foo.AppImage?secret"),
            ("linux-x86_64", "%2e%2e.AppImage"), ("linux-x86_64", "install.exe"),
            ("unknown", "test.AppImage"), ("darwin-aarch64", "installer.dmg"),
        ):
            with self.subTest(filename=filename), self.assertRaises(publisher.InvalidRelease):
                publisher.artifact_name(platform, filename)

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaises(publisher.InvalidRelease):
            publisher.strict_json(b'{"version":"0.1.3","version":"9.0.0"}')


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
        self.artifact.write_bytes(b"isolated ShaCraft updater fixture; not an executable")
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


if __name__ == "__main__":
    unittest.main()
