"""Verify build inventory without a container daemon or runtime policy changes."""

import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location(
    "package_models", ROOT / "packages/polymarket-bot/scripts/package_models.py"
)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


class ModelPackagingTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = self.root / packager.MANIFEST
        self.manifest.parent.mkdir(parents=True)
        self.output = self.root / "output"
        self.bundle = self.root / packager.CATALOG / "selected"
        self.bundle.mkdir(parents=True)
        self.data = b'{"estimator":"unchanged"}\n'
        self.vectors = b'[]\n'
        self.entry = {"model_key": "selected", "artifact_sha256": hashlib.sha256(self.data).hexdigest()}
        self.write_manifest([self.entry])
        self.bundle_manifest = {
            "model_key": "selected", "model_file": "model.json",
            "model_sha256": self.entry["artifact_sha256"],
            "golden_vectors_file": "golden-vectors.json",
            "golden_vectors_sha256": hashlib.sha256(self.vectors).hexdigest(),
            "deployment_scope": "paper_only", "live_capital_allowed": False,
        }
        self.write_bundle_manifest()
        (self.bundle / "model.json").write_bytes(self.data)
        (self.bundle / "golden-vectors.json").write_bytes(self.vectors)
        (self.bundle / "scratch.txt").write_text("not a declared runtime file")
        unused = self.root / packager.CATALOG / "unused"
        unused.mkdir()
        (unused / "model.json").write_text("excluded")

    def write_manifest(self, models):
        self.manifest.write_text(json.dumps({"schema_version": 1, "models": models}))

    def write_bundle_manifest(self):
        (self.bundle / "manifest.json").write_text(json.dumps(self.bundle_manifest))

    def test_exact_selected_files_and_manifest_digest(self):
        digest = packager.package(self.root, self.output)
        self.assertEqual(digest, hashlib.sha256(self.manifest.read_bytes()).hexdigest())
        self.assertEqual(
            sorted(str(p.relative_to(self.output)) for p in self.output.rglob("*") if p.is_file()),
            ["model-manifest.json", "selected/golden-vectors.json", "selected/manifest.json", "selected/model.json"],
        )
        for name in ("model.json", "manifest.json", "golden-vectors.json"):
            self.assertEqual((self.output / "selected" / name).read_bytes(), (self.bundle / name).read_bytes())

    def test_development_ignores_manifest_and_preserves_full_catalog(self):
        self.manifest.write_text("invalid production manifest has no effect on development")
        result = subprocess.run(
            ["python3", str(ROOT / "packages/polymarket-bot/scripts/package_models.py"),
             "package", str(self.output), "--root", str(self.root)],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.output / "model-manifest.json").exists())
        self.assertEqual({p.name for p in self.output.iterdir()}, {"selected", "unused"})
        for source in (self.root / packager.CATALOG).rglob("*"):
            if source.is_file():
                self.assertEqual(source.read_bytes(), (self.output / source.relative_to(self.root / packager.CATALOG)).read_bytes())

    def test_production_cli_uses_manifest_explicitly(self):
        result = subprocess.run(
            ["python3", str(ROOT / "packages/polymarket-bot/scripts/package_models.py"),
             "package", str(self.output), "--root", str(self.root), "--environment", "production"],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({p.name for p in self.output.iterdir() if p.is_dir()}, {"selected"})

    def test_missing_bundle_and_hash_mismatch_fail_before_writing(self):
        for name in ("model.json", "golden-vectors.json"):
            with self.subTest(name=name):
                source = self.bundle / name
                original = source.read_bytes()
                source.write_bytes(b"changed")
                with self.assertRaisesRegex(ValueError, "hash mismatch"):
                    packager.package(self.root, self.output)
                self.assertFalse(self.output.exists())
                source.write_bytes(original)
        (self.bundle / "model.json").unlink()
        with self.assertRaises(FileNotFoundError):
            packager.package(self.root, self.output)
        self.assertFalse(self.output.exists())

    def test_duplicates_paths_and_identity_mismatch_are_rejected(self):
        self.write_manifest([self.entry, self.entry])
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            packager.package(self.root, self.output)
        self.write_manifest([{**self.entry, "model_key": "../outside"}])
        with self.assertRaisesRegex(ValueError, "path component"):
            packager.package(self.root, self.output)
        self.write_manifest([self.entry])
        self.bundle_manifest["model_key"] = "different"
        self.write_bundle_manifest()
        with self.assertRaisesRegex(ValueError, "identity differs"):
            packager.package(self.root, self.output)

    def test_stale_destination_is_not_modified(self):
        self.output.mkdir()
        old = self.output / "old-model"
        old.write_bytes(b"preserve")
        with self.assertRaisesRegex(ValueError, "must be empty"):
            packager.package(self.root, self.output)
        self.assertEqual(old.read_bytes(), b"preserve")

    def test_bundle_policy_metadata_is_preserved_without_interpretation(self):
        self.bundle_manifest["deployment_scope"] = "arbitrary-existing-policy"
        self.bundle_manifest["production_qualified"] = False
        self.write_bundle_manifest()
        packager.package(self.root, self.output)
        self.assertEqual(json.loads((self.output / "selected/manifest.json").read_bytes()), self.bundle_manifest)

    def test_git_inputs_ignore_excluded_models_and_track_selected_files(self):
        def git(*args):
            return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.DEVNULL)

        def commit():
            git("add", ".")
            git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")

        git("init", "-q")
        commit()
        original = packager.fingerprint(self.root, "HEAD")
        (self.root / packager.CATALOG / "unused/model.json").write_text("changed excluded model")
        commit()
        self.assertEqual(packager.fingerprint(self.root, "HEAD"), original)
        (self.bundle / "golden-vectors.json").write_bytes(b"[1]")
        commit()
        self.assertNotEqual(packager.fingerprint(self.root, "HEAD"), original)
        self.manifest.unlink()
        commit()
        self.assertEqual(packager.input_paths(self.root, "HEAD"), [packager.CATALOG])

    def test_real_manifest_packages_only_production_pair(self):
        expected = {
            "btc-5m-conservative-selective-paper-20260917",
            "btc-5m-conservative-selective-development-live-pilot-20260921-v1",
        }
        packager.package(ROOT, self.output)
        self.assertEqual({p.name for p in self.output.iterdir() if p.is_dir()}, expected)
        definitions = [
            "btc-5m-conservative-selective-paper-20260917.json",
            "btc-5m-conservative-selective-live-pilot-20260921.json",
        ]
        for name in definitions:
            definition = json.loads((ROOT / "infra/processes" / name).read_bytes())
            metadata = definition["metadata"]
            data = (self.output / metadata["model_key"] / "model.json").read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), metadata["model_artifact_sha256"])


if __name__ == "__main__":
    unittest.main()
