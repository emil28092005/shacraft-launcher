#!/usr/bin/env python3
"""Explicit workflow-only GitHub release operations, with fail-closed gates."""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import release

ENVIRONMENTS = {"launcher-release", "launcher-release-publish"}


def gh(*args, missing=False):
    result = subprocess.run(["gh", *args], capture_output=True, text=True, timeout=900, check=False)
    if result.returncode:
        try:
            body = json.loads(result.stdout)
        except ValueError:
            body = {}
        if missing and str(body.get("status")) == "404":
            return None
        raise ValueError("GitHub operation failed; no permission or validation bypass is allowed")
    return result.stdout


def api(path, missing=False):
    value = gh("api", f"repos/{release.REPOSITORY}/{path}", missing=missing)
    return None if value is None else json.loads(value)


def validate_environment(data):
    release.require(isinstance(data, dict), "protected environment is absent")
    policy = data.get("deployment_branch_policy") or {}
    release.require(
        policy.get("protected_branches") is True and policy.get("custom_branch_policies") is False,
        "release environment must allow protected branches only",
    )
    rules = data.get("protection_rules") or []
    reviewers = next((rule for rule in rules if rule.get("type") == "required_reviewers"), {})
    release.require(
        reviewers.get("prevent_self_review") is True, "environment must prevent self review"
    )
    allowed = reviewers.get("reviewers") or []
    release.require(
        any(
            item.get("type") in {"User", "Team"}
            and type((item.get("reviewer") or {}).get("id")) is int
            and item["reviewer"]["id"] > 0
            for item in allowed
        ),
        "release environment requires an independent reviewer",
    )


def workflow_guard():
    release.require(
        os.environ.get("GITHUB_REPOSITORY") == release.REPOSITORY, "wrong release repository"
    )
    release.require(
        os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch",
        "release must be dispatched manually",
    )
    release.require(
        os.environ.get("GITHUB_REF") == "refs/heads/main", "release workflow must run from main"
    )


def gate(environment):
    workflow_guard()
    release.require(environment in ENVIRONMENTS, "unexpected release environment")
    release.require(
        api("branches/main").get("protected") is True, "main must be a protected branch"
    )
    validate_environment(api(f"environments/{environment}", missing=True))
    # Jobs consume this output only after validation. Never reference a missing
    # environment directly: GitHub would create it without protection rules.
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(f"environment={environment}\n")


def asset_snapshot(data):
    release.require(
        data.get("draft") is True and data.get("prerelease") is False,
        "expected a stable DRAFT release",
    )
    assets = data.get("assets") or []
    result = {}
    for asset in assets:
        name = asset["name"]
        release.require(
            name not in result and asset.get("state") == "uploaded",
            "duplicate or incomplete release asset",
        )
        result[name] = (asset["id"], asset["size"], asset.get("digest"))
    return result


def verify_draft(root, version, tag):
    release.version_tag(version, tag)
    before = api(f"releases/tags/{tag}")
    release.require(before.get("tag_name") == tag, "draft tag mismatch")
    snapshot = asset_snapshot(before)
    packages = release.expected_names(version)
    expected = packages | {name + ".sig" for name in packages} | {"latest.json", "latest.json.sig"}
    release.require(set(snapshot) == expected, "draft asset set is incomplete or unexpected")
    # Reject remote names and declared sizes before gh writes any asset locally.
    # This is only a pre-download bound; signatures and actual bytes are still
    # checked below, followed by the remote identity/race check.
    for name, (_, size, _) in snapshot.items():
        limit = (
            32 * 1024 if name == "latest.json"
            else 8 * 1024 if name.endswith(".sig")
            else release.MAX_SIZE
        )
        release.require(type(size) is int and 0 < size <= limit, "invalid remote draft asset size")
    with tempfile.TemporaryDirectory(prefix="shacraft-draft-check-") as temporary:
        directory = Path(temporary)
        gh("release", "download", tag, "--repo", release.REPOSITORY, "--dir", str(directory))
        release.verify_release(root, directory, version, tag)
        release.require(
            set(snapshot) == {p.name for p in directory.iterdir()},
            "draft assets changed during download",
        )
        for path in directory.iterdir():
            release.require(snapshot[path.name][1] == path.stat().st_size, "draft size mismatch")
    after = api(f"releases/tags/{tag}")
    release.require(
        after["id"] == before["id"] and asset_snapshot(after) == snapshot,
        "draft changed during verification",
    )
    return after


def draft(root, directory, version, tag):
    workflow_guard()
    release.preflight(root, version, tag, check_git=True, signing=True)
    release.verify_release(root, directory, version, tag)
    with tempfile.TemporaryDirectory(prefix="shacraft-release-notes-") as temporary:
        notes = Path(temporary) / "notes.md"
        release_notes = release.read_json(directory / "latest.json")["notes"]
        instructions = (
            f"https://github.com/{release.REPOSITORY}/blob/{tag}/docs/updater-release.md"
            "#installed-package-migration-and-recovery"
        )
        notes.write_text(
            release_notes + "\n\n" +
            "[Установка, переход с 0.1.1 и восстановление](" + instructions + ").\n" +
            "Используйте прежний тип пакета. Обновление .deb выполняется через менеджер пакетов.\n",
            encoding="utf-8",
        )
        gh(
            "release",
            "create",
            tag,
            *[str(path) for path in sorted(directory.iterdir())],
            "--repo",
            release.REPOSITORY,
            "--draft",
            "--verify-tag",
            "--title",
            f"ShaCraft Launcher {version}",
            "--notes-file",
            str(notes),
        )
    verify_draft(root, version, tag)
    print(
        "Complete draft uploaded and re-verified. Publication requires the separate protected workflow."
    )


def publish(root, version, tag, confirmation):
    workflow_guard()
    release.require(
        confirmation == f"publish {tag}", "explicit publication confirmation does not match tag"
    )
    release.preflight(root, version, tag, check_git=True)
    pinned = (root / "src-tauri/updater-public-key.txt").read_text().strip()
    release.require(
        release.public_key() == pinned, "publication key differs from committed updater key"
    )
    gate("launcher-release-publish")  # Re-check protection immediately before publication.
    current = api("releases/latest", missing=True)
    if current is not None:
        old_tag = current.get("tag_name", "")
        release.version_tag(old_tag.removeprefix("v"), old_tag)
        release.require(
            tuple(map(int, version.split("."))) > tuple(map(int, old_tag[1:].split("."))),
            "publication must advance the stable release version",
        )
    verify_draft(root, version, tag)
    gh("release", "edit", tag, "--repo", release.REPOSITORY, "--draft=false", "--latest")
    print("Verified release published by explicit protected operator workflow.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    sub = parser.add_subparsers(dest="command", required=True)
    item = sub.add_parser("gate")
    item.add_argument("environment", choices=sorted(ENVIRONMENTS))
    for name in ("draft", "publish"):
        item = sub.add_parser(name)
        item.add_argument("--version", required=True)
        item.add_argument("--tag", required=True)
        if name == "draft":
            item.add_argument("--directory", required=True, type=Path)
        else:
            item.add_argument("--confirmation", required=True)
    args = parser.parse_args()
    if args.command == "gate":
        gate(args.environment)
    elif args.command == "draft":
        draft(args.root.resolve(), args.directory, args.version, args.tag)
    else:
        publish(args.root.resolve(), args.version, args.tag, args.confirmation)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, TypeError) as error:
        print(f"Release stopped: {error}", file=sys.stderr)
        sys.exit(1)
