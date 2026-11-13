# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Unit tests of the installer release's guard, verification and notes.

    python3 -m unittest discover -s scripts/release-installer/test -v
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))
ROOT = HERE.parent.parent.parent

import guard  # noqa: E402
import make_fake_medium  # noqa: E402
import notes  # noqa: E402
import verify  # noqa: E402

REV = "6f4cbebe39ee8a3b7fa5a291202ae2c64db130b1"

ALL_GOOD = {
    "EVENT": "workflow_dispatch",
    "REF": "refs/heads/main",
    "PUBLISH_INPUT": "true",
    "RELEASES_ENABLED": "1",
    "HAVE_ENDPOINT": "true",
    "HAVE_BUCKET": "true",
    "HAVE_ACCESS_KEY": "true",
    "HAVE_SECRET_KEY": "true",
    "PUBLIC_URL": "https://tur-downloads.us-ord-1.linodeobjects.com",
    "REDISTRIBUTABLE": "yes",
    "TAG_FREE": "yes",
}


class GuardTest(unittest.TestCase):
    def test_all_conditions_publish(self):
        self.assertEqual(guard.decide(ALL_GOOD), (True, []))

    def test_each_condition_alone_stops_publishing(self):
        for name in ALL_GOOD:
            for bad in ("", "false", "0", "TRUE", "True", "no", "1 2", "refs/heads/mainx"):
                env = {**ALL_GOOD, name: bad}
                with self.subTest(name=name, value=bad):
                    publish, reasons = guard.decide(env)
                    self.assertFalse(publish)
                    self.assertEqual(len(reasons), 1)

    def test_missing_variables_mean_dry_run(self):
        publish, reasons = guard.decide({})
        self.assertFalse(publish)
        self.assertEqual(len(reasons), len(guard.RULES))

    def test_defaults_of_a_manual_start_are_a_dry_run(self):
        # publish is false by default; the variable and secrets do not exist yet.
        env = {"EVENT": "workflow_dispatch", "REF": "refs/heads/main", "PUBLISH_INPUT": "false",
               "RELEASES_ENABLED": "", "HAVE_ENDPOINT": "false", "HAVE_BUCKET": "false",
               "HAVE_ACCESS_KEY": "false", "HAVE_SECRET_KEY": "false", "PUBLIC_URL": "",
               "REDISTRIBUTABLE": "no", "TAG_FREE": "yes"}
        self.assertFalse(guard.decide(env)[0])

    def test_public_url_must_be_https(self):
        for url in ("http://tur.example", "https://", "ftp://x", "https://a b", "https://x/$(id)"):
            with self.subTest(url=url):
                self.assertFalse(guard.decide({**ALL_GOOD, "PUBLIC_URL": url})[0])


class VerifyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        make_fake_medium.make(self.tmp, REV, mib=2)
        self.medium, self.record = self.tmp / "medium", self.tmp / "record"

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def refused(self, revision=REV, version="0.1.0-preview.1"):
        with self.assertRaises(verify.Refused) as caught:
            verify.verify(self.medium, self.record, revision, version)
        return str(caught.exception)

    def test_good_medium_is_laid_out(self):
        facts = verify.verify(self.medium, self.record, REV, "0.1.0-preview.1")
        facts = verify.lay_out(facts, self.medium, self.tmp / "out")
        out = self.tmp / "out"
        self.assertEqual(facts["iso"], "tur-0.1.0-preview.1-x86_64.iso")
        self.assertTrue((out / facts["iso"]).is_file())
        self.assertFalse((self.medium / "tur-installer.iso").exists(), "the medium is moved, not copied")
        sums = (out / facts["sums"]).read_text().splitlines()
        self.assertEqual(sums[0], f"{facts['iso_sha256']}  {facts['iso']}")
        self.assertEqual(json.loads((out / "release.json").read_text())["redistributable"], "yes")

    def test_corrupted_medium(self):
        with (self.medium / "tur-installer.iso").open("r+b") as handle:
            handle.seek(1000)
            handle.write(b"\0" * 8)
        self.assertIn("SHA-256", self.refused())

    def test_record_differs(self):
        (self.record / "medium" / "tur-installer.SHA256SUMS").write_text("0" * 64 + "  tur-installer.iso\n")
        self.assertIn("test record", self.refused())

    def test_record_missing(self):
        shutil.rmtree(self.record / "medium")
        self.assertIn("test record", self.refused())

    def test_wrong_revision(self):
        self.assertIn("not from the run's commit", self.refused(revision="a" * 40))

    def test_dirty_build(self):
        manifest = self.medium / "tur-installer.manifest.json"
        data = json.loads(manifest.read_text())
        data["source"]["build_id"] += ".dirty"
        manifest.write_text(json.dumps(data))
        # the checksum no longer matches either; fix it to reach the build ID check
        import hashlib
        iso_line = (self.medium / "tur-installer.SHA256SUMS").read_text().splitlines()[0]
        sums = f"{iso_line}\n{hashlib.sha256(manifest.read_bytes()).hexdigest()}  {manifest.name}\n"
        for folder in (self.medium, self.record / "medium"):
            (folder / "tur-installer.SHA256SUMS").write_text(sums)
        shutil.copyfile(manifest, self.record / "medium" / manifest.name)
        self.assertIn("not a clean build", self.refused())

    def test_versions(self):
        for version in ("0.1.0", "v0.1.0-preview.1", "0.1.0-preview..1", "0.1.0-pre view", "01.1.0-x"):
            with self.subTest(version=version):
                self.assertIn("not a pre-release version", self.refused(version=version))
        self.assertIn("does not match", self.refused(version="0.2.0-preview.1"))

    def test_not_redistributable_is_recorded(self):
        shutil.rmtree(self.tmp)
        make_fake_medium.make(self.tmp, REV, mib=1, redistributable=False)
        facts = verify.verify(self.medium, self.record, REV, "0.1.0-preview.1")
        self.assertEqual(facts["redistributable"], "no")
        self.assertFalse(guard.decide({**ALL_GOOD, "REDISTRIBUTABLE": facts["redistributable"]})[0])


class NotesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        make_fake_medium.make(self.tmp, REV, mib=1)
        facts = verify.verify(self.tmp / "medium", self.tmp / "record", REV, "0.1.0-preview.1")
        verify.lay_out(facts, self.tmp / "medium", self.tmp / "out")
        self.out = self.tmp / "out"
        self.env = dict(os.environ)

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self.env)
        shutil.rmtree(self.tmp)

    def render(self, signing: str, **env) -> str:
        (self.out / "signing.env").write_text(signing)
        os.environ.update({"GITHUB_SERVER_URL": "https://github.com", "GITHUB_REPOSITORY": "giorgiogiudice/TUR",
                           "SOURCE_RUN_URL": "https://github.com/giorgiogiudice/TUR/actions/runs/1",
                           "RELEASE_DATE": "2026-11-13", **env})
        known = notes.known_issues_body((ROOT / "docs/releases/known-issues.md").read_text())
        return notes.render((ROOT / "docs/releases/TEMPLATE.md").read_text(), notes.values(self.out, known))

    def test_published_notes(self):
        identity = "https://github.com/giorgiogiudice/TUR/.github/workflows/release-installer.yml@refs/heads/main"
        text = self.render(f"keyless=yes\nkeyless_identity={identity}\nkeyless_issuer="
                           "https://token.actions.githubusercontent.com\nrelease_key=no\n",
                           DRY_RUN="false", TUR_DOWNLOADS_PUBLIC_URL="https://dl.example.org/")
        self.assertNotIn("{{", text)
        self.assertNotIn("DRY RUN", text)
        self.assertNotIn("SPDX", text)
        self.assertIn("https://dl.example.org/releases/0.1.0-preview.1/tur-0.1.0-preview.1-x86_64.iso", text)
        self.assertIn("sha256sum -c --ignore-missing tur-0.1.0-preview.1-x86_64.SHA256SUMS", text)
        self.assertIn(f"--certificate-identity {identity}", text)
        self.assertNotIn("release-key", text)
        for words in ("without any warranty", "DD Image mode", "Fedora Media Writer", "conv=fsync",
                      "not for use by or for a government", "not provided, endorsed or supported by the Fedora Project",
                      "/blob/v0.1.0-preview.1/docs/guides/install-on-a-test-pc.md", "#64", "20261112.g6f4cbebe39ee",
                      "Not tested yet"):
            self.assertIn(words, text)

    def test_dry_run_and_release_key(self):
        text = self.render("keyless=no\nrelease_key=yes\n", DRY_RUN="true", TUR_DOWNLOADS_PUBLIC_URL="")
        self.assertIn("DRY RUN", text)
        self.assertIn("(not uploaded: dry run)", text)
        self.assertIn("--key tur-0.1.0-preview.1-x86_64.release-key.pub", text)
        self.assertNotIn("--certificate-identity", text)

    def test_unknown_names_stop(self):
        with self.assertRaises(SystemExit):
            notes.render("{{nonsense}}", {"a": "b"})
        with self.assertRaises(SystemExit):
            notes.render("{{#nonsense}}x{{/nonsense}}", {"a": "b"})


if __name__ == "__main__":
    unittest.main()
