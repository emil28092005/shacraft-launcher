"""Execute the actual trusted workflow gate against disposable local Git history."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(os.environ.get("RELEASE_TEST_ROOT", Path(__file__).resolve().parents[1]))
WORKFLOWS = [ROOT / ".github/workflows" / name for name in ("release.yml", "release-publish.yml")]
GATE = "      - name: Resolve tag using trusted workflow Git commands\n"


def workflow_gate(workflow):
    after = workflow.read_text().split(GATE, 1)[1]
    lines = after.split("        run: |\n", 1)[1].splitlines()
    body = []
    for line in lines:
        if not line.startswith("          "):
            break
        body.append(line[10:])
    return "\n".join(body) + "\n"


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="shacraft-source-gate-")
        self.directory = Path(self.temporary.name)
        self.environment = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Release Gate Test")
        (self.directory / "reviewed.txt").write_text("Reviewed source\n")
        self.git("add", ".")
        self.git("commit", "-m", "Reviewed main commit")
        self.good = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.good)
        self.git("tag", "v0.2.0")
        self.git("checkout", "-b", "unreviewed")
        (self.directory / "scripts").mkdir()
        # This validator would falsely accept its own tag if a workflow ran it.
        (self.directory / "scripts/release.py").write_text(
            "from pathlib import Path\nPath('untrusted-code-ran').write_text('bypassed')\n"
        )
        self.git("add", ".")
        self.git("commit", "-m", "Unreviewed tag with false validator")
        self.bad = self.git("rev-parse", "HEAD")
        self.git("tag", "v9.9.9")
        self.git("checkout", "main")

    def tearDown(self):
        self.temporary.cleanup()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.directory,
                                       env=self.environment, text=True, stderr=subprocess.DEVNULL).strip()

    def run_gate(self, workflow, tag):
        output = self.directory / (workflow.stem + "-output")
        output.unlink(missing_ok=True)
        result = subprocess.run(["bash", "-c", workflow_gate(workflow)], cwd=self.directory,
                                env=dict(self.environment, RELEASE_TAG=tag, GITHUB_OUTPUT=str(output)),
                                capture_output=True, text=True, check=False)
        return result, output.read_text() if output.exists() else ""

    def test_main_tag_emits_immutable_commit(self):
        for workflow in WORKFLOWS:
            with self.subTest(workflow=workflow.name):
                result, output = self.run_gate(workflow, "v0.2.0")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, f"commit={self.good}\n")

    def test_unreviewed_tag_cannot_replace_its_own_ancestry_validator(self):
        for workflow in WORKFLOWS:
            with self.subTest(workflow=workflow.name):
                result, output = self.run_gate(workflow, "v9.9.9")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")
                self.assertFalse((self.directory / "untrusted-code-ran").exists())

    def test_missing_or_retargeted_tag_cannot_change_validated_source(self):
        for workflow in WORKFLOWS:
            with self.subTest(workflow=workflow.name):
                result, output = self.run_gate(workflow, "v8.8.8")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")
                self.git("tag", "-f", "v0.2.0", self.good)
                result, output = self.run_gate(workflow, "v0.2.0")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.git("tag", "-f", "v0.2.0", self.bad)
                self.assertEqual(output, f"commit={self.good}\n")
                result, fresh_output = self.run_gate(workflow, "v0.2.0")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(fresh_output, "")

    def test_workflows_execute_candidate_code_only_after_gate_and_checkout_sha(self):
        for workflow in WORKFLOWS:
            with self.subTest(workflow=workflow.name):
                text = workflow.read_text()
                preflight = text.split("  preflight:\n", 1)[1].split("\n  build:\n", 1)[0].split("\n  publish:\n", 1)[0]
                before, after = preflight.split(GATE, 1)
                self.assertEqual(before.count("uses: actions/checkout@v4"), 1)
                self.assertIn("fetch-depth: 0", before)
                self.assertNotIn("ref:", before)  # Initial checkout is trusted workflow main.
                self.assertIn("release.version_tag", before)
                self.assertLess(after.index("git merge-base --is-ancestor"), after.index("uses: actions/checkout@v4"))
                self.assertIn("ref: ${{ steps.source.outputs.commit }}", after)
                self.assertNotIn("ref: ${{ inputs.tag }}", text)
                downstream = text[len(text.split("  preflight:\n", 1)[0]) + len("  preflight:\n") + len(preflight):]
                self.assertGreater(downstream.count("ref: ${{ needs.preflight.outputs.commit }}"), 0)
                self.assertEqual(downstream.count("uses: actions/checkout@v4"), downstream.count("ref: ${{ needs.preflight.outputs.commit }}"))
                self.assertEqual(text.count("uses: actions/checkout@v4"), text.count("persist-credentials: false"))

    def test_write_token_is_only_exposed_to_explicit_final_publish_step(self):
        text = WORKFLOWS[1].read_text().split("\n  publish:\n", 1)[1]
        job_environment = re.search(r"(?m)^    env:\n((?:      .*\n)+)", text)
        self.assertIsNotNone(job_environment)
        self.assertNotIn("GH_TOKEN", job_environment.group(1))
        before, final = text.split("      - name: Re-download, verify signatures/metadata/assets and explicitly publish\n", 1)
        self.assertNotIn("GH_TOKEN", before)
        self.assertIn("GH_TOKEN: ${{ github.token }}", final)


if __name__ == "__main__":
    unittest.main()
