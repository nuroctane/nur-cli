"""Failure cases: overwritten local edits, missing resources, alias loss,
symlink escapes, nondeterministic indexes, and needless file replacement.
"""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import subprocess
import json
import sys
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
credentials = load("check_vendored_credentials")


class MaintenanceTests(unittest.TestCase):
    def test_embedded_credentials_are_detected_without_disclosing_values(self):
        # Match inside a minified artifact and an example command, without
        # including any actual credentials in the repository test fixture.
        google = b"AIza" + b"A" * 35
        tailscale = b"tskey-" + b"auth-" + b"a" * 12
        self.assertEqual(list(credentials.findings(b'var env={apiKey:"' + google + b'"};\nexport KEY=' + tailscale)),
                         [("Google API key", 1), ("Tailscale key", 2)])
        self.assertEqual(list(credentials.findings(b'apiKey:process.env.KEY; TS_AUTHKEY=$CI_SECRET')), [])

    def test_offline_renderer_strips_cloud_config_and_recomputes_artifacts(self):
        import hashlib
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "upstream"
            renderer = source / "lib/diagram-render"
            (renderer / "dist").mkdir(parents=True)
            (renderer / "src").mkdir()
            (renderer / "scripts").mkdir()
            (renderer / "src/entry.ts").write_text("window.render = exportToSvg;")
            (renderer / "scripts/build.ts").write_text("build offline renderer")
            (renderer / "dist/diagram-render.html").write_text(
                "VITE_APP_FIREBASE_CONFIG:'{\"apiKey\":\"sample-cloud-key\",\"projectId\":\"upstream\"}',render:exportToSvg")
            (renderer / "dist/BUILD_INFO.json").write_text(json.dumps({"deps":{"renderer":"1"}}))
            resources = sync.prepared_resources(source, root / "skills/gstack")
            bundle = resources["lib/diagram-render/dist/diagram-render.html"]
            info = json.loads(resources["lib/diagram-render/dist/BUILD_INFO.json"])
            self.assertEqual(bundle, b'VITE_APP_FIREBASE_CONFIG:"{}",render:exportToSvg')
            self.assertEqual(sync.offline_diagram_bundle(bundle), bundle)
            self.assertEqual(info["sha256"], hashlib.sha256(bundle).hexdigest())
            self.assertEqual(info["bytes"], len(bundle))
            self.assertEqual(info["srcSha256"], hashlib.sha256(b"window.render = exportToSvg;build offline renderer").hexdigest())
            self.assertEqual(info["deps"], {"renderer":"1"})

    def test_reviewed_resource_override_survives_upstream_refresh(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "upstream"
            source.mkdir()
            (source / "SKILL.md").write_text("upstream examples")
            overlay = root / "scripts/skill-overrides/demo.md"
            overlay.parent.mkdir(parents=True)
            overlay.write_text("reviewed detection examples")
            destination = root / "skills/demo"
            with patch.object(sync, "REPO", root), patch.object(sync, "SKILLS", root / "skills"):
                previous, _ = sync.refresh_tree(source, destination, {}, overrides={"SKILL.md":"scripts/skill-overrides/demo.md"})
                (source / "SKILL.md").write_text("new upstream examples")
                sync.refresh_tree(source, destination, previous, overrides={"SKILL.md":"scripts/skill-overrides/demo.md"})
                self.assertEqual((destination / "SKILL.md").read_text(), "reviewed detection examples")
                with self.assertRaises(ValueError):
                    sync.refresh_tree(source, destination, previous, overrides={"SKILL.md":"upstream/SKILL.md"})

    def test_duplicate_name_precedence_is_portable_across_folder_case(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for folder, description in [("a", "First guide"), ("B", "Second guide")]:
                path = root / folder
                path.mkdir()
                (path / "SKILL.md").write_text(f"---\nname: shared\ndescription: {description}\n---\nBody")
            entries = generate.generate([root])["skills"]
            self.assertEqual(len(entries), 1)
            self.assertEqual(entries[0]["description"], "First guide")
            self.assertEqual(entries[0]["aliases"], ["a"])

    def test_staged_artifact_detects_clean_filter_changes(self):
        with patch.dict(sys.modules, {"sync_upstream_skills":sync}):
            check = load("check_skill_snapshot")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            source = root / "skills/demo/README.md"
            source.parent.mkdir(parents=True)
            source.write_text("upstream guide\n")
            manifest = {"skills":{"demo":{"files":{"README.md":sync.digest(source)},"executables":[]}}}
            (root / "skills/upstream-lock.json").write_text(json.dumps(manifest))
            subprocess.run(["git", "-C", str(root), "add", "."], check=True, capture_output=True)
            with patch.object(check, "ROOT", root):
                self.assertEqual(check.main(), 0)
                altered = subprocess.run(["git", "-C", str(root), "hash-object", "-w", "--stdin"],
                                         input=b"altered by a clean filter\n", capture_output=True, check=True).stdout.decode().strip()
                subprocess.run(["git", "-C", str(root), "update-index", "--cacheinfo", "100644", altered,
                                "skills/demo/README.md"], check=True)
                self.assertEqual(check.main(), 1)
            self.assertEqual(source.read_text(), "upstream guide\n")

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
