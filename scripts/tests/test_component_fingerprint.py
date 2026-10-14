"""Verify that component fingerprints track production image inputs only."""

import pathlib
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "ci/component-fingerprint.py"


class ComponentFingerprintTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = pathlib.Path(self.temp.name)
        for name in (
            ".dockerignore", "Cargo.toml", "Cargo.lock",
            "packages/polymarket-bot/Cargo.toml",
            "packages/market-data-ingester/Cargo.toml",
            "packages/market-data-ingester/build.rs",
            "packages/market-data-ingester/src/main.rs",
            "packages/market-data-ingester/Dockerfile.production",
            "packages/market-data-ingester/README.md",
            "common/proto/market.proto",
            "scripts/ci/component-fingerprint.py",
        ):
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"{name}\n")
        shutil.copy2(SCRIPT, self.repo / "scripts/ci/component-fingerprint.py")
        self.git("init", "-q", "-b", "test")
        self.commit(".")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, check=True,
                              text=True, capture_output=True).stdout.strip()

    def commit(self, path):
        self.git("add", path)
        self.git("-c", "user.name=CI test", "-c", "user.email=ci@example.invalid",
                 "commit", "-qm", "fixture")

    def fingerprint(self):
        return subprocess.run(
            ["python3", "scripts/ci/component-fingerprint.py", "ingester"],
            cwd=self.repo, check=True, text=True, capture_output=True,
        ).stdout.strip()

    def change(self, relative):
        path = self.repo / relative
        path.write_text(path.read_text() + "changed\n")
        self.commit(relative)

    def test_documentation_does_not_change_ingester_fingerprint(self):
        before = self.fingerprint()
        self.change("packages/market-data-ingester/README.md")
        self.assertEqual(self.fingerprint(), before)

    def test_lockfile_changes_ingester_fingerprint(self):
        before = self.fingerprint()
        self.change("Cargo.lock")
        self.assertNotEqual(self.fingerprint(), before)

    def test_source_and_dockerfile_change_ingester_fingerprint(self):
        for relative in (
            "packages/market-data-ingester/src/main.rs",
            "packages/market-data-ingester/Dockerfile.production",
        ):
            with self.subTest(path=relative):
                before = self.fingerprint()
                self.change(relative)
                self.assertNotEqual(self.fingerprint(), before)


if __name__ == "__main__":
    unittest.main()
