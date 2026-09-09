"""No network: reject unsafe draft listings before invoking GitHub download."""

import copy
import unittest
from pathlib import Path
from unittest.mock import patch

import release
import release_github


class DraftAssetGateTests(unittest.TestCase):
    def draft(self):
        packages = release.expected_names("0.3.0")
        names = packages | {name + ".sig" for name in packages} | {"latest.json", "latest.json.sig"}
        self.assertEqual(len(names), 18)
        return {
            "id": 123, "tag_name": "v0.3.0", "draft": True, "prerelease": False,
            "assets": [{"name": name, "id": index, "state": "uploaded", "size": 1, "digest": None}
                       for index, name in enumerate(sorted(names), 1)],
        }

    def assert_no_download(self, data):
        with (patch.object(release_github, "api", return_value=data),
              patch.object(release_github, "gh") as github,
              patch.object(release, "verify_release") as verify):
            with self.assertRaises(ValueError):
                release_github.verify_draft(Path("/unused"), "0.3.0", "v0.3.0")
            github.assert_not_called()
            verify.assert_not_called()

    def fake_download(self, listing):
        def download(*args):
            self.assertEqual(args[:3], ("release", "download", "v0.3.0"))
            destination = Path(args[args.index("--dir") + 1])
            for asset in listing["assets"]:
                (destination / asset["name"]).write_bytes(b"x" * asset["size"])
        return download

    def test_exact_complete_listing_still_downloads_and_verifies_actual_contents(self):
        listing = self.draft()
        with (patch.object(release_github, "api", side_effect=[listing, listing]) as api,
              patch.object(release_github, "gh", side_effect=self.fake_download(listing)) as github,
              patch.object(release, "verify_release") as verify):
            self.assertEqual(release_github.verify_draft(Path("/unused"), "0.3.0", "v0.3.0"), listing)
            github.assert_called_once()
            verify.assert_called_once()
            self.assertEqual(api.call_count, 2)

    def test_unexpected_traversal_absolute_and_excessive_names_cannot_download(self):
        for name in ["extra.exe", "../latest.json", r"..\latest.json", "/tmp/latest.json",
                     "folder/latest.json", "latest.JSON", "x" * 4096]:
            with self.subTest(name=name[:50]):
                listing = self.draft()
                listing["assets"][0]["name"] = name
                self.assert_no_download(listing)
        listing = self.draft()
        listing["assets"].append({"name": "extra.txt", "id": 100, "state": "uploaded", "size": 1})
        self.assert_no_download(listing)

    def test_missing_duplicate_and_unfinished_files_cannot_download(self):
        for index in range(18):
            with self.subTest(missing=index):
                listing = self.draft()
                listing["assets"].pop(index)
                self.assert_no_download(listing)
        listing = self.draft()
        listing["assets"].append(copy.deepcopy(listing["assets"][0]))
        self.assert_no_download(listing)
        listing = self.draft()
        listing["assets"][0]["state"] = "new"
        self.assert_no_download(listing)

    def test_every_asset_size_must_be_a_positive_integer_within_its_limit(self):
        listing = self.draft()
        for index, asset in enumerate(listing["assets"]):
            limit = 32768 if asset["name"] == "latest.json" else 8192 if asset["name"].endswith(".sig") else 1024**3
            for size in [0, -1, True, False, 1.0, "1", None, limit + 1]:
                with self.subTest(name=asset["name"], size=size):
                    altered = copy.deepcopy(listing)
                    altered["assets"][index]["size"] = size
                    self.assert_no_download(altered)

    def test_declared_size_boundaries_are_accepted_without_allocating_large_files(self):
        listing = self.draft()
        for asset in listing["assets"]:
            asset["size"] = 32768 if asset["name"] == "latest.json" else 8192 if asset["name"].endswith(".sig") else 1024**3
        with (patch.object(release_github, "api", return_value=listing),
              patch.object(release_github, "gh", side_effect=RuntimeError("download boundary reached")) as github):
            with self.assertRaisesRegex(RuntimeError, "download boundary reached"):
                release_github.verify_draft(Path("/unused"), "0.3.0", "v0.3.0")
            github.assert_called_once()

    def test_valid_listing_does_not_bypass_signature_failure_or_remote_race_checks(self):
        listing = self.draft()
        with (patch.object(release_github, "api", return_value=listing) as api,
              patch.object(release_github, "gh", side_effect=self.fake_download(listing)) as github,
              patch.object(release, "verify_release", side_effect=ValueError("invalid updater signature")) as verify):
            with self.assertRaisesRegex(ValueError, "invalid updater signature"):
                release_github.verify_draft(Path("/unused"), "0.3.0", "v0.3.0")
            github.assert_called_once()
            verify.assert_called_once()
            self.assertEqual(api.call_count, 1)
        changed = copy.deepcopy(listing)
        changed["assets"][0]["id"] += 100
        with (patch.object(release_github, "api", side_effect=[listing, changed]),
              patch.object(release_github, "gh", side_effect=self.fake_download(listing)),
              patch.object(release, "verify_release") as verify):
            with self.assertRaisesRegex(ValueError, "changed during verification"):
                release_github.verify_draft(Path("/unused"), "0.3.0", "v0.3.0")
            verify.assert_called_once()


if __name__ == "__main__":
    unittest.main()
