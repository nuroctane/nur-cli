"""Failure cases: overwritten local edits, missing resources, alias loss,
symlink escapes, nondeterministic indexes, and needless file replacement.
"""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import subprocess
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


sync = load("sync_upstream_skills")
install = load("install_skill_snapshot")
generate = load("generate_skill_intents")


class MaintenanceTests(unittest.TestCase):
    def test_resource_hashes_survive_windows_checkout_line_endings(self):
        self.assertEqual(sync.content_digest(b"first\r\nsecond\r\n"),
                         sync.content_digest(b"first\nsecond\n"))
        self.assertNotEqual(sync.content_digest(b"\xff\r\n"), sync.content_digest(b"\xff\n"))

    def test_executable_resources_keep_upstream_git_modes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / "guide.md").write_text("guide")
            (root / "run.sh").write_text("#!/bin/sh\necho fixture\n")
            subprocess.run(["git", "-C", str(root), "add", "."], check=True, capture_output=True)
            subprocess.run(["git", "-C", str(root), "update-index", "--chmod=+x", "run.sh"], check=True)
            self.assertEqual(sync.executable_resources(root), frozenset({"run.sh"}))

    def test_upstream_refresh_retains_local_edits_and_complete_resources(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, destination = root / "source", root / "skills/demo"
            source.mkdir()
            destination.mkdir(parents=True)
            (source / "SKILL.md").write_text("new instructions")
            (source / "references").mkdir()
            (source / "references/guide.md").write_text("new guide")
            (destination / "SKILL.md").write_text("local instructions")
            with patch.object(sync, "SKILLS", root / "skills"):
                managed, retained = sync.refresh_tree(source, destination, {})
            self.assertEqual(retained, ["SKILL.md"])
            self.assertEqual((destination / "SKILL.md").read_text(), "local instructions")
            self.assertEqual((destination / "references/guide.md").read_text(), "new guide")
            self.assertIn("references/guide.md", managed)

    def test_installed_snapshot_updates_owned_files_and_backs_them_up(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source/demo"
            source.mkdir(parents=True)
            (source / "SKILL.md").write_text("one")
            with patch.object(install, "SKILLS", root / "source"):
                install.deploy(root / "home", {"demo": source}, {}, True, root / "backups")
                (source / "SKILL.md").write_text("two")
                report = install.deploy(root / "home", {"demo": source}, {}, True, root / "backups")
                self.assertEqual(report["updated"], ["demo/SKILL.md"])
                self.assertEqual(next((root / "backups").rglob("SKILL.md")).read_text(), "one")
                installed = root / "home/demo/SKILL.md"
                timestamp = installed.stat().st_mtime_ns
                install.deploy(root / "home", {"demo": source}, {}, True, root / "backups")
                self.assertEqual(installed.stat().st_mtime_ns, timestamp)
                installed.write_text("custom")
                report = install.deploy(root / "home", {"demo": source}, {}, True, root / "backups")
                self.assertEqual(report["preserved"], ["demo/SKILL.md"])
                self.assertEqual(installed.read_text(), "custom")

    def test_target_cannot_escape_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                install.safe_target(Path(temporary) / "skills", "../outside")

    def test_explicit_vendor_refresh_backs_up_differing_copies(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source/demo"
            source.mkdir(parents=True)
            (source / "SKILL.md").write_text("reviewed upstream instructions")
            target = root / "home/demo/SKILL.md"
            target.parent.mkdir(parents=True)
            target.write_text("previous pack copy")
            (target.parent / "personal.md").write_text("local addition")
            with patch.object(install, "SKILLS", root / "source"):
                report = install.deploy(root / "home", {"demo":source}, {}, True, root / "backups", {"demo/SKILL.md"})
            self.assertEqual(report["updated"], ["demo/SKILL.md"])
            self.assertEqual(next((root / "backups").rglob("SKILL.md")).read_text(), "previous pack copy")
            self.assertEqual((target.parent / "personal.md").read_text(), "local addition")

    def test_index_is_deterministic_and_keeps_folder_aliases(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            skill = root / "craft"
            skill.mkdir()
            (skill / "SKILL.md").write_text("---\nname: craft-ui-skills\ndescription: >-\n  Interface craft\n  and composition\n---\nBody")
            first = generate.generate([root])
            self.assertEqual(first, generate.generate([root]))
            entry = first["skills"][0]
            self.assertEqual(entry["aliases"], ["craft"])
            self.assertIn("/craft", entry["triggers"])
            self.assertEqual(entry["description"], "Interface craft and composition")


if __name__ == "__main__":
    unittest.main()
