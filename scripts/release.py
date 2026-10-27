#!/usr/bin/env python3
"""Manual version preparation and shared native release metadata (Python 3.11+)."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent
VERSION = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-(alpha|beta|rc)\.([1-9][0-9]*))?"
)


def version_info(version, build_number=1):
    """Keep prereleases ordered before finals in both Linux package managers."""
    match = VERSION.fullmatch(version)
    if not match or any(int(part) > 65535 for part in match.groups()[:3]):
        raise ValueError("Use MAJOR.MINOR.PATCH[-alpha.N|-beta.N|-rc.N] (components <= 65535)")
    if not 1 <= build_number <= 9999:
        raise ValueError("Build number must be between 1 and 9999 for native version fields")
    base = ".".join(match.groups()[:3])
    return {
        "version": version,
        "tag": f"v{version}",
        "base_version": base,
        "build_number": str(build_number),
        "windows_version": f"{base}.{build_number}",
        "linux_version": version.replace("-", "~"),
        "prerelease": "true" if match.group(4) else "false",
    }


def metadata(root=ROOT, tag=None, build_number=1):
    with (root / "Cargo.toml").open("rb") as source:
        package = tomllib.load(source)["package"]
    info = version_info(package["version"], build_number)
    with (root / "Cargo.lock").open("rb") as source:
        locked = tomllib.load(source)["package"]
    if not any(p["name"] == "dosty" and p["version"] == info["version"] for p in locked):
        raise ValueError("Cargo.lock does not match Cargo.toml; run cargo update --offline --workspace")
    if tag is not None and tag != info["tag"]:
        raise ValueError(f"Tag {tag!r} does not match {info['tag']!r} in Cargo.toml")
    return info


def prepare(version):
    version_info(version)
    manifest = ROOT / "Cargo.toml"
    lockfile = ROOT / "Cargo.lock"
    original = manifest.read_text(encoding="utf-8")
    original_lock = lockfile.read_bytes()
    current = tomllib.loads(original)["package"]["version"]
    updated, count = re.subn(
        rf'(?m)^(version\s*=\s*)"{re.escape(current)}"',
        lambda match: f'{match.group(1)}"{version}"', original, count=1,
    )
    if count != 1:
        raise ValueError("Could not locate the package version in Cargo.toml")
    try:
        manifest.write_text(updated, encoding="utf-8")
        subprocess.run(["cargo", "update", "--offline", "--workspace"], cwd=ROOT, check=True)
        metadata()
    except (OSError, ValueError, subprocess.CalledProcessError):
        manifest.write_text(original, encoding="utf-8")
        lockfile.write_bytes(original_lock)
        raise
    print(f"Prepared {version}. Review Cargo.toml/Cargo.lock and add docs/releases/{version}.md.")


def verify_tag(tag):
    """Release only the checked-out, existing tag after its commit reaches main."""
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    tagged = subprocess.check_output(
        ["git", "rev-parse", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT, text=True,
    ).strip()
    if tagged != commit:
        raise ValueError("The checked-out commit does not match the release tag")
    subprocess.run(["git", "merge-base", "--is-ancestor", commit, "origin/main"], cwd=ROOT, check=True)


def copy_licenses(destination):
    destination.mkdir(parents=True, exist_ok=True)
    for source, name in [
        (ROOT / "LICENSE", "LICENSE.txt"),
        (ROOT / "assets/fonts/OFL.txt", "Newsreader-OFL.txt"),
        (ROOT / "branding/dosty/sources/DMSans-OFL.txt", "DM-Sans-OFL.txt"),
        (ROOT / "target/release-notices.html", "THIRD-PARTY-NOTICES.html"),
    ]:
        shutil.copyfile(source, destination / name)


def linux_package_filename(package_format, version):
    """Use SemVer in downloads; tildes belong only in native package metadata."""
    version_info(version)
    if package_format == "deb":
        return f"dosty_{version}-1_amd64.deb"
    if package_format == "rpm":
        return f"dosty-{version}-1.x86_64.rpm"
    raise ValueError(f"Unsupported Linux package format: {package_format}")


def release_assets(directory, version):
    names = [
        f"Dosty-{version}-macos-arm64.dmg",
        f"Dosty-{version}-macos-x86_64.dmg",
        f"Dosty-{version}-windows-x86_64-setup.exe",
        linux_package_filename("deb", version),
        linux_package_filename("rpm", version),
        f"dosty-{version}-source.tar.gz",
    ]
    paths = [directory / name for name in names]
    for path in paths:
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {path.name}")
    return paths


def file_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checksums(directory, version):
    paths = release_assets(directory, version)
    lines = []
    for path in paths:
        lines.append(f"{file_digest(path)}  {path.name}\n")
    output = directory / "SHA256SUMS"
    output.write_text("".join(lines), encoding="utf-8")
    return paths + [output]


def verify_release_upload(info, assets):
    # The public /releases/tags endpoint does not expose unpublished drafts.
    api_url = subprocess.check_output([
        "gh", "release", "view", info["tag"], "--json", "apiUrl", "--jq", ".apiUrl",
    ], cwd=ROOT, text=True).strip()
    uploaded = json.loads(subprocess.check_output(["gh", "api", api_url], cwd=ROOT, text=True))
    if (uploaded.get("tag_name") != info["tag"] or uploaded.get("draft") is not True
            or uploaded.get("published_at") is not None):
        raise ValueError("Uploaded release must be an unpublished draft of the expected tag")
    if uploaded.get("prerelease") != (info["prerelease"] == "true"):
        raise ValueError("Uploaded release has the wrong prerelease flag")
    remote = {asset["name"]: asset for asset in uploaded["assets"]}
    expected = {path.name for path in assets}
    if set(remote) != expected or len(uploaded["assets"]) != len(expected):
        raise ValueError(
            f"Uploaded asset names differ from checksum inputs: "
            f"missing {sorted(expected - set(remote))}; unexpected {sorted(set(remote) - expected)}"
        )
    for path in assets:
        asset = remote[path.name]
        if asset.get("state") != "uploaded" or asset.get("size") != path.stat().st_size:
            raise ValueError(f"Incomplete uploaded asset: {path.name}")
        if asset.get("digest") != f"sha256:{file_digest(path)}":
            raise ValueError(f"Checksum mismatch for uploaded asset: {path.name}")
    print(f"Verified {len(assets)} uploaded assets; {info['tag']} remains an unpublished draft.")


def create_draft(directory, info):
    assets = checksums(directory, info["version"])
    # gh refuses an existing release, including a draft. Never clobber assets or publish.
    command = [
        "gh", "release", "create", info["tag"], "--verify-tag", "--draft",
        "--title", f"Dosty {info['version']}",
        "--notes-file", str(ROOT / f"docs/releases/{info['version']}.md"),
    ]
    if info["prerelease"] == "true":
        command.append("--prerelease")
    subprocess.run(command + [str(path) for path in assets], cwd=ROOT, check=True)
    verify_release_upload(info, assets)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("prepare").add_argument("version")
    versions = commands.add_parser("metadata")
    versions.add_argument("--tag")
    versions.add_argument("--require-tag", action="store_true")
    versions.add_argument("--github-output", type=Path)
    commands.add_parser("licenses").add_argument("directory", type=Path)
    commands.add_parser("checksums").add_argument("directory", type=Path)
    draft = commands.add_parser("draft")
    draft.add_argument("directory", type=Path)
    draft.add_argument("--tag", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.version)
    elif args.command == "licenses":
        copy_licenses(args.directory)
    else:
        info = metadata(tag=getattr(args, "tag", None), build_number=int(os.environ.get("DOSTY_BUILD_NUMBER", "1")))
        if args.command == "metadata":
            if args.require_tag:
                if not args.tag:
                    raise ValueError("--require-tag requires --tag")
                verify_tag(args.tag)
            notes = ROOT / f"docs/releases/{info['version']}.md"
            if not notes.is_file():
                raise ValueError(f"Missing release notes: {notes.relative_to(ROOT)}")
            info["commit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
            if args.github_output:
                with args.github_output.open("a", encoding="utf-8") as output:
                    output.writelines(f"{key}={value}\n" for key, value in info.items())
            print(json.dumps(info, indent=2))
        elif args.command == "checksums":
            checksums(args.directory, info["version"])
        elif args.command == "draft":
            verify_tag(info["tag"])
            create_draft(args.directory, info)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
