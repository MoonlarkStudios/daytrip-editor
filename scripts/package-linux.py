#!/usr/bin/env python3
"""Package an already-built x86_64 binary using the native Debian or RPM tools."""

import argparse
from pathlib import Path
import platform
import shutil
import subprocess

from release import ROOT, copy_licenses, metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("format", choices=["deb", "rpm"])
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("Linux packages currently require a native x86_64 Linux build")
    info = metadata()
    workspace = ROOT / "target/package-linux" / args.format
    if workspace.exists():
        shutil.rmtree(workspace)
    stage = workspace / ("debian/dosty" if args.format == "deb" else "stage")
    for source, destination in [
        ("target/release/dosty", "usr/bin/dosty"),
        ("packaging/linux/app.dosty.editor.desktop", "usr/share/applications/app.dosty.editor.desktop"),
        ("branding/dosty/icons/png/dosty-256.png", "usr/share/icons/hicolor/256x256/apps/app.dosty.editor.png"),
    ]:
        output = stage / destination
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / source, output)
        output.chmod(0o755 if destination == "usr/bin/dosty" else 0o644)
    licenses = stage / ("usr/share/doc/dosty" if args.format == "deb" else "usr/share/licenses/dosty")
    copy_licenses(licenses)
    subprocess.run(["desktop-file-validate", str(stage / "usr/share/applications/app.dosty.editor.desktop")], check=True)
    distribution = ROOT / "target/dist"
    distribution.mkdir(parents=True, exist_ok=True)
    version = info["linux_version"]
    if args.format == "deb":
        # debhelper metadata is needed by dpkg-shlibdeps even for a prebuilt binary.
        debian = workspace / "debian"
        debian.mkdir(exist_ok=True)
        (debian / "control").write_text("Source: dosty\n\nPackage: dosty\nArchitecture: any\nDescription: Dosty\n", encoding="utf-8")
        dependencies = subprocess.check_output(
            ["dpkg-shlibdeps", "-O", str((stage / "usr/bin/dosty").relative_to(workspace))], cwd=workspace, text=True,
        ).strip().removeprefix("shlibs:Depends=")
        if not dependencies:
            raise ValueError("dpkg-shlibdeps returned no runtime dependencies")
        control = (ROOT / "packaging/linux/control").read_text(encoding="utf-8")
        control = control.replace("@VERSION@", version).replace("@SHLIBS@", dependencies)
        (stage / "DEBIAN").mkdir()
        (stage / "DEBIAN/control").write_text(control, encoding="utf-8")
        (licenses / "copyright").write_text(
            "Dosty\nCopyright 2026 Dosty contributors\nLicense: GPL-3.0-or-later\n\n"
            + (ROOT / "LICENSE").read_text(encoding="utf-8"), encoding="utf-8",
        )
        package = distribution / f"dosty_{version}-1_amd64.deb"
        subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(stage), str(package)], check=True)
        subprocess.run(["dpkg-deb", "--info", str(package)], check=True)
    else:
        spec = workspace / "dosty.spec"
        spec.write_text((ROOT / "packaging/linux/dosty.spec").read_text(encoding="utf-8").replace("@VERSION@", version), encoding="utf-8")
        subprocess.run([
            "rpmbuild", "-bb", "--define", f"_topdir {workspace}",
            "--define", f"dosty_stage {stage}", str(spec),
        ], check=True)
        package = distribution / f"dosty-{version}-1.x86_64.rpm"
        shutil.copyfile(workspace / "RPMS/x86_64" / package.name, package)
        subprocess.run(["rpm", "-qip", str(package)], check=True)
    print(package)


if __name__ == "__main__":
    main()
