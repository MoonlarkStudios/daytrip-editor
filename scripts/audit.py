#!/usr/bin/env python3
"""Keep RustSec findings visible and enforce narrowly reviewed maintenance exceptions."""

import datetime
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
POLICY = ROOT / ".cargo" / "audit-exceptions.json"


def review(report, exceptions, today):
    """Return printable findings and policy failures; never exempt vulnerabilities."""
    findings, errors, allowed, seen = [], [], {}, set()
    for entry in exceptions:
        key = (entry["advisory"], entry["package"], entry["version"])
        if key in allowed or not entry["reason"].strip():
            raise ValueError(f"Invalid or duplicate exception: {key}")
        deadline = datetime.date.fromisoformat(entry["review_by"])
        if today > deadline:
            errors.append(f"Exception expired on {deadline}: {' / '.join(key)}")
        allowed[key] = entry

    # An external cargo-audit config must not hide findings from this policy.
    if report["settings"]["ignore"]:
        errors.append("cargo-audit ignore list must be empty")
    vulnerabilities = report["vulnerabilities"]
    if vulnerabilities["found"] or vulnerabilities["count"] or vulnerabilities["list"]:
        errors.append("RustSec reported vulnerabilities; exceptions cannot permit them")
    for vulnerability in vulnerabilities["list"]:
        findings.append(f"VULNERABILITY: {vulnerability['advisory']['id']} "
                        f"{vulnerability['package']['name']} {vulnerability['package']['version']}")

    for kind, warnings in report["warnings"].items():
        for warning in warnings:
            package = warning["package"]
            advisory = warning.get("advisory") or {}
            key = (advisory.get("id", kind), package["name"], package["version"])
            label = f"{kind}: {' / '.join(key)}"
            if (kind == "unmaintained" and warning["kind"] == kind
                    and advisory.get("informational") == kind and key in allowed):
                seen.add(key)
                entry = allowed[key]
                findings.append(f"REVIEWED {label} (review by {entry['review_by']})\n"
                                f"  {entry['reason']}")
            else:
                findings.append(f"UNREVIEWED {label}")
                errors.append(f"Unapproved finding: {label}")
    for key in allowed.keys() - seen:
        errors.append(f"Remove or re-review stale exception: {' / '.join(key)}")
    return findings, errors


def main():
    try:
        result = subprocess.run(
            ["cargo", "audit", "--json", "--deny", "yanked"],
            cwd=ROOT, capture_output=True, text=True, check=False,
        )
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="")
        report = json.loads(result.stdout)
        exceptions = json.loads(POLICY.read_text())
        findings, errors = review(report, exceptions, datetime.date.today())
        print("\n".join(findings))
        if result.returncode:
            errors.append(f"cargo audit failed with exit status {result.returncode}")
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        if errors:
            return 1
        print(f"Audit passed with {len(exceptions)} explicit maintenance exceptions.")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Audit could not complete: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
