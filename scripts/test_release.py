"""Regression coverage for version ordering and draft release safety boundaries."""

from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def test_prerelease_native_versions(self):
        info = release.version_info("0.1.0-alpha.1", 42)
        self.assertEqual(info["tag"], "v0.1.0-alpha.1")
        self.assertEqual(info["linux_version"], "0.1.0~alpha.1")
        self.assertEqual(info["base_version"], "0.1.0")
        self.assertEqual(info["windows_version"], "0.1.0.42")
        self.assertEqual(info["prerelease"], "true")
        for version in ["0.1.0-beta.2", "0.1.0-rc.3"]:
            self.assertEqual(release.version_info(version)["linux_version"], version.replace("-", "~"))

    def test_final_version(self):
        info = release.version_info("1.2.3")
        self.assertEqual(info["linux_version"], "1.2.3")
        self.assertEqual(info["prerelease"], "false")

    def test_linux_download_names_preserve_semver_and_package_ordering(self):
        for version in ["0.1.0-alpha.1", "0.1.0-beta.2", "0.1.0-rc.3", "1.2.3"]:
            with self.subTest(version=version):
                self.assertEqual(release.linux_package_filename("deb", version), f"dosty_{version}-1_amd64.deb")
                self.assertEqual(release.linux_package_filename("rpm", version), f"dosty-{version}-1.x86_64.rpm")
                for package_format in ["deb", "rpm"]:
                    self.assertRegex(release.linux_package_filename(package_format, version), r"^[A-Za-z0-9._-]+$")
                self.assertEqual(release.version_info(version)["linux_version"], version.replace("-", "~"))

    def test_reject_unsupported_or_unsafe_native_versions(self):
        for version in ["v0.1.0", "01.1.0", "0.1", "0.1.0-alpha", "0.1.0-alpha.0",
                        "0.1.0-alpha.01", "0.1.0+build.1", "65536.0.0", "0.1.0\ncontents=write"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.version_info(version)
        for number in [0, -1, 10000]:
            with self.subTest(number=number), self.assertRaises(ValueError):
                release.version_info("0.1.0", number)

    def test_manifest_lockfile_and_tag_must_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\nname = "dosty"\nversion = "0.1.0-alpha.1"\n')
            (root / "Cargo.lock").write_text('[[package]]\nname = "dosty"\nversion = "0.1.0"\n')
            with self.assertRaisesRegex(ValueError, "Cargo.lock"):
                release.metadata(root)
            (root / "Cargo.lock").write_text('[[package]]\nname = "dosty"\nversion = "0.1.0-alpha.1"\n')
            with self.assertRaisesRegex(ValueError, "Tag"):
                release.metadata(root, tag="v0.1.0")
            self.assertEqual(release.metadata(root, tag="v0.1.0-alpha.1")["version"], "0.1.0-alpha.1")

    def test_missing_or_empty_artifact_prevents_any_release_write(self):
        with tempfile.TemporaryDirectory() as directory, patch("release.subprocess.run") as run:
            with self.assertRaises(ValueError):
                release.create_draft(Path(directory), release.version_info("0.1.0-alpha.1"))
            run.assert_not_called()

    def test_tag_must_match_head_and_have_reached_main(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def git(*arguments):
                subprocess.run([
                    "git", "-c", "user.name=Release test", "-c", "user.email=release@example.invalid",
                    "-c", "commit.gpgsign=false", *arguments,
                ], cwd=root, check=True, stdout=subprocess.DEVNULL)

            git("init", "--quiet", "--initial-branch=main")
            git("commit", "--quiet", "--allow-empty", "-m", "Initial")
            git("tag", "v0.1.0")
            git("update-ref", "refs/remotes/origin/main", "HEAD")
            with patch("release.ROOT", root):
                release.verify_tag("v0.1.0")
                git("commit", "--quiet", "--allow-empty", "-m", "Unmerged")
                with self.assertRaisesRegex(ValueError, "checked-out commit"):
                    release.verify_tag("v0.1.0")
                git("tag", "v0.1.1")
                with self.assertRaises(subprocess.CalledProcessError):
                    release.verify_tag("v0.1.1")

    def test_checksums_cover_all_packages_and_source(self):
        version = "0.1.0-alpha.1"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = [f"Dosty-{version}-macos-arm64.dmg", f"Dosty-{version}-macos-x86_64.dmg",
                     f"Dosty-{version}-windows-x86_64-setup.exe", "dosty_0.1.0-alpha.1-1_amd64.deb",
                     "dosty-0.1.0-alpha.1-1.x86_64.rpm", f"dosty-{version}-source.tar.gz"]
            for name in names:
                (root / name).write_bytes(b"abc")
            paths = release.checksums(root, version)
            lines = (root / "SHA256SUMS").read_text().splitlines()
            self.assertEqual(len(paths), 7)
            self.assertEqual({line.split("  ")[1] for line in lines}, set(names))
            self.assertTrue(all(line.startswith("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ") for line in lines))
            (root / names[0]).write_bytes(b"")
            with self.assertRaises(ValueError):
                release.checksums(root, version)

    def test_release_creation_is_always_draft(self):
        for version, prerelease in [("0.1.0-alpha.1", True), ("0.1.0", False)]:
            with self.subTest(version=version), patch("release.checksums", return_value=[Path("package")]), patch("release.verify_release_upload"), patch("release.subprocess.run") as run:
                release.create_draft(Path("target/dist"), release.version_info(version))
                command = run.call_args.args[0]
                self.assertEqual(command[:3], ["gh", "release", "create"])
                self.assertIn("--draft", command)
                self.assertIn("--verify-tag", command)
                self.assertEqual("--prerelease" in command, prerelease)
                self.assertNotIn("--clobber", command)


class ReleaseUploadTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.info = release.version_info("0.1.0-alpha.1")
        names = [
            "Dosty-0.1.0-alpha.1-macos-arm64.dmg",
            "Dosty-0.1.0-alpha.1-macos-x86_64.dmg",
            "Dosty-0.1.0-alpha.1-windows-x86_64-setup.exe",
            "dosty_0.1.0-alpha.1-1_amd64.deb",
            "dosty-0.1.0-alpha.1-1.x86_64.rpm",
            "dosty-0.1.0-alpha.1-source.tar.gz",
        ]
        for name in names:
            (self.root / name).write_bytes(b"abc")
        self.assets = release.checksums(self.root, self.info["version"])
        self.uploaded = {
            "tag_name": self.info["tag"], "draft": True, "published_at": None, "prerelease": True,
            "assets": [{"name": path.name, "size": path.stat().st_size, "state": "uploaded",
                        "digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()}
                       for path in reversed(self.assets)],
        }

    def api_responses(self, uploaded):
        return patch("release.subprocess.check_output", side_effect=[
            "https://api.github.com/repos/example/dosty/releases/1\n", json.dumps(uploaded),
        ])

    def test_uploaded_names_and_digests_match_every_checksum_entry(self):
        with self.api_responses(self.uploaded) as api:
            release.verify_release_upload(self.info, self.assets)
        self.assertEqual(api.call_args_list[1].args[0], [
            "gh", "api", "https://api.github.com/repos/example/dosty/releases/1",
        ])
        uploaded = {asset["name"]: asset["digest"] for asset in self.uploaded["assets"]}
        for line in (self.root / "SHA256SUMS").read_text().splitlines():
            digest, name = line.split("  ", 1)
            self.assertEqual(uploaded[name], "sha256:" + digest)

    def test_server_renaming_fails_the_draft_job(self):
        for package_format in ["deb", "rpm"]:
            uploaded = deepcopy(self.uploaded)
            asset = next(a for a in uploaded["assets"] if a["name"].endswith("." + package_format))
            asset["name"] = asset["name"].replace("-alpha.1", ".alpha.1")
            with self.subTest(format=package_format), self.api_responses(uploaded), patch("release.subprocess.run") as run:
                with self.assertRaisesRegex(ValueError, "Uploaded asset names differ"):
                    release.create_draft(self.root, self.info)
                self.assertEqual(run.call_count, 1)
                self.assertIn("--draft", run.call_args.args[0])

    def test_missing_extra_and_duplicate_assets_fail(self):
        for change in ["missing", "extra", "duplicate"]:
            uploaded = deepcopy(self.uploaded)
            if change == "missing":
                uploaded["assets"].pop()
            else:
                additional = dict(uploaded["assets"][0])
                if change == "extra":
                    additional["name"] = "unexpected.txt"
                uploaded["assets"].append(additional)
            with self.subTest(change=change), self.api_responses(uploaded):
                with self.assertRaisesRegex(ValueError, "Uploaded asset names differ"):
                    release.verify_release_upload(self.info, self.assets)

    def test_upload_state_size_and_digest_must_match(self):
        for field, value, error in [("state", "starter", "Incomplete"), ("size", 0, "Incomplete"),
                                    ("digest", "sha256:" + "0" * 64, "Checksum mismatch")]:
            uploaded = deepcopy(self.uploaded)
            uploaded["assets"][0][field] = value  # SHA256SUMS is also verified.
            with self.subTest(field=field), self.api_responses(uploaded):
                with self.assertRaisesRegex(ValueError, error + ".*SHA256SUMS"):
                    release.verify_release_upload(self.info, self.assets)

    def test_wrong_release_state_or_tag_fails(self):
        for field, value in [("draft", False), ("published_at", "2026-10-26T00:00:00Z"),
                             ("tag_name", "v0.2.0"), ("prerelease", False)]:
            uploaded = dict(self.uploaded, **{field: value})
            with self.subTest(field=field), self.api_responses(uploaded):
                with self.assertRaises(ValueError):
                    release.verify_release_upload(self.info, self.assets)

    def test_legacy_tilde_names_are_rejected_before_upload(self):
        for package_format in ["deb", "rpm"]:
            path = self.root / release.linux_package_filename(package_format, self.info["version"])
            path.rename(path.with_name(path.name.replace("-alpha.1", "~alpha.1")))
        with patch("release.subprocess.run") as run:
            with self.assertRaisesRegex(ValueError, "Missing or empty release asset"):
                release.create_draft(self.root, self.info)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
