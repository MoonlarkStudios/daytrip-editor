"""Regression coverage for version ordering and draft release safety boundaries."""

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
                     f"Dosty-{version}-windows-x86_64-setup.exe", "dosty_0.1.0~alpha.1-1_amd64.deb",
                     "dosty-0.1.0~alpha.1-1.x86_64.rpm", f"dosty-{version}-source.tar.gz"]
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
            with self.subTest(version=version), patch("release.checksums", return_value=[Path("package")]), patch("release.subprocess.run") as run:
                release.create_draft(Path("target/dist"), release.version_info(version))
                command = run.call_args.args[0]
                self.assertEqual(command[:3], ["gh", "release", "create"])
                self.assertIn("--draft", command)
                self.assertIn("--verify-tag", command)
                self.assertEqual("--prerelease" in command, prerelease)
                self.assertNotIn("--clobber", command)


if __name__ == "__main__":
    unittest.main()
