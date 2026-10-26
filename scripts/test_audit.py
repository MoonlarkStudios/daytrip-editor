"""Exercise fail-closed policy boundaries with synthetic cargo-audit reports."""

import copy
import datetime
import unittest

from audit import review


class AuditPolicyTests(unittest.TestCase):
    def setUp(self):
        self.today = datetime.date(2026, 10, 25)
        self.exception = {
            "advisory": "RUSTSEC-2026-0001", "package": "example", "version": "1.2.3",
            "review_by": "2027-01-23", "reason": "Reviewed upstream dependency.",
        }
        self.warning = {
            "kind": "unmaintained", "package": {"name": "example", "version": "1.2.3"},
            "advisory": {"id": "RUSTSEC-2026-0001", "informational": "unmaintained"},
        }
        self.report = {
            "settings": {"ignore": []},
            "vulnerabilities": {"found": False, "count": 0, "list": []},
            "warnings": {"unmaintained": [self.warning]},
        }

    def errors(self):
        return review(self.report, [self.exception], self.today)[1]

    def test_exact_reviewed_warning_is_visible_and_allowed(self):
        findings, errors = review(self.report, [self.exception], self.today)
        self.assertFalse(errors)
        self.assertIn("REVIEWED unmaintained", findings[0])

    def test_changed_version_or_new_advisory_fails(self):
        for section, field, value in [("package", "version", "1.2.4"),
                                      ("advisory", "id", "RUSTSEC-2026-0002")]:
            with self.subTest(field=field):
                report = copy.deepcopy(self.report)
                report["warnings"]["unmaintained"][0][section][field] = value
                self.assertTrue(review(report, [self.exception], self.today)[1])

    def test_unsoundness_and_yanked_releases_cannot_be_exempted(self):
        for kind in ["unsound", "yanked"]:
            with self.subTest(kind=kind):
                self.warning["kind"] = kind
                self.warning["advisory"]["informational"] = kind
                self.report["warnings"] = {kind: [self.warning]}
                self.assertTrue(self.errors())

    def test_vulnerability_cannot_be_exempted(self):
        self.report["vulnerabilities"] = {"found": True, "count": 1, "list": [self.warning]}
        self.assertTrue(self.errors())

    def test_expired_and_stale_exceptions_fail(self):
        self.assertTrue(review(self.report, [self.exception], datetime.date(2027, 1, 24))[1])
        self.report["warnings"] = {}
        self.assertTrue(self.errors())

    def test_external_ignore_list_fails(self):
        self.report["settings"]["ignore"] = [self.exception["advisory"]]
        self.assertTrue(self.errors())

    def test_missing_report_fields_fail_closed(self):
        with self.assertRaises(KeyError):
            review({}, [self.exception], self.today)


if __name__ == "__main__":
    unittest.main()
