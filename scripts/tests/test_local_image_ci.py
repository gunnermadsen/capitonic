"""Exercise local image CI provenance without a running container daemon."""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "local-image-ci.sh"


class LocalImageCiTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.repo / "scripts").mkdir()
        shutil.copy2(SCRIPT, self.repo / "scripts/local-image-ci.sh")
        for path in (
            "Cargo.toml",
            "Cargo.lock",
            ".dockerignore",
            "packages/polymarket-bot/Cargo.toml",
            "packages/polymarket-bot/Dockerfile.production",
            "packages/polymarket-bot/src/main.rs",
            "packages/market-data-ingester/Cargo.toml",
            "packages/market-data-ingester/Dockerfile.production",
            "packages/market-data-ingester/src/main.rs",
            "packages/db-migrate/package.json",
            "packages/db-migrate/package-lock.json",
            "packages/db-migrate/tsconfig.json",
            "packages/db-migrate/Dockerfile.production",
            "packages/db-migrate/src/main.ts",
        ):
            target = self.repo / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(path + "\n")
        runner = self.bin / "nerdctl"
        runner.write_text('''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
if args[:2] == ['--namespace', 'k8s.io']:
    args = args[2:]
state_file = pathlib.Path(os.environ['FAKE_IMAGE_STATE'])
state = json.loads(state_file.read_text()) if state_file.exists() else {}
if args[0] == 'info':
    sys.exit(0)
if args[:2] == ['image', 'inspect']:
    name = args[-1]
    if name not in state:
        sys.exit(1)
    record = state[name]
    if '--format' in args:
        fmt = args[args.index('--format') + 1]
        if fmt == '{{ .Id }}':
            print(record['id'])
        elif 'revision' in fmt:
            print(record['revision'])
        elif 'version' in fmt:
            print(record['version'])
        else:
            sys.exit(2)
    else:
        print(json.dumps(record))
elif args[:2] == ['image', 'tag']:
    state[args[3]] = state[args[2]]
elif args[:2] == ['image', 'rm']:
    state.pop(args[2], None)
elif args[0] == 'build':
    if os.environ.get('FAKE_BUILD_FAIL') == '1':
        sys.exit(1)
    name = args[args.index('--tag') + 1]
    revision = args[args.index('--build-arg') + 1].split('=', 1)[1]
    version = args[args.index('--label') + 1].split('=', 1)[1]
    state[name] = {'id': 'sha256:' + hashlib.sha256((name + revision).encode()).hexdigest(),
                   'revision': revision, 'version': version}
    count = pathlib.Path(os.environ['FAKE_BUILD_COUNT'])
    count.write_text(str(int(count.read_text()) + 1))
else:
    sys.exit(2)
state_file.write_text(json.dumps(state))
''')
        runner.chmod(0o755)
        checker = ('#!/bin/sh\n'
                   'printf "%s %s\\n" "$(basename "$0")" "$*" >> "$FAKE_CHECK_LOG"\n'
                   '[ "${FAKE_CHECK_FAIL:-0}" != 1 ]\n')
        for name in ("cargo", "npm", "node"):
            tool = self.bin / name
            tool.write_text(checker)
            tool.chmod(0o755)
        self.state = self.root / "images.json"
        self.count = self.root / "build-count"
        self.count.write_text("0")
        self.check_log = self.root / "checks.log"
        self.check_log.write_text("")
        self.env = os.environ.copy()
        self.env.update(PATH=f"{self.bin}:{self.env['PATH']}", FAKE_IMAGE_STATE=str(self.state),
                        FAKE_BUILD_COUNT=str(self.count), FAKE_CHECK_LOG=str(self.check_log),
                        GIT_AUTHOR_NAME="CI test",
                        GIT_AUTHOR_EMAIL="ci-test@example.invalid", GIT_COMMITTER_NAME="CI test",
                        GIT_COMMITTER_EMAIL="ci-test@example.invalid")
        self.git("init", "-q", "-b", "defect/local-ci-test")
        self.commit()

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, env=self.env,
                              text=True, capture_output=True, check=True).stdout.strip()

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "fixture input")

    def run_ci(self, component, **env):
        return subprocess.run(["bash", "scripts/local-image-ci.sh", component, "--build"],
                              cwd=self.repo, env={**self.env, **env}, text=True,
                              capture_output=True)

    def tags(self, component):
        return self.git("tag", "--list", f"image/{component}/*").splitlines()

    def test_all_components_build_once_then_reuse_on_unchanged_inputs(self):
        inputs = {"polymarket-bot": "packages/polymarket-bot/src/main.rs",
                  "ingester": "packages/market-data-ingester/src/main.rs",
                  "db-migrate": "packages/db-migrate/src/main.ts"}
        for component, path in inputs.items():
            with self.subTest(component=component):
                prior_builds = int(self.count.read_text())
                first = self.run_ci(component)
                self.assertEqual(first.returncode, 0, first.stderr)
                expected_base = {"polymarket-bot": "3.2.2", "ingester": "1.2.1",
                                 "db-migrate": "0.2.0"}[component]
                self.assertIn(f"image/{component}/v{expected_base}-local.0",
                              self.tags(component))
                if component == "polymarket-bot":
                    self.assertIn("cargo +1.92.0 clippy --locked --package polymarket-bot --all-targets -- -D warnings",
                                  self.check_log.read_text())
                elif component == "ingester":
                    self.assertIn("cargo +1.92.0 clippy --locked --package market-data-ingester --all-targets --all-features -- -D warnings",
                                  self.check_log.read_text())
                self.assertEqual(len(self.tags(component)), 2)
                self.assertEqual(self.count.read_text(), str(prior_builds + 1))
                repeated = self.run_ci(component)
                self.assertEqual(repeated.returncode, 0, repeated.stderr)
                self.assertIn("no new image or tags were created", repeated.stdout)
                self.assertEqual(self.count.read_text(), str(prior_builds + 1))
                self.assertEqual(len(self.tags(component)), 2)
                (self.repo / path).write_text("changed\n")
                self.commit()
                changed = self.run_ci(component)
                self.assertEqual(changed.returncode, 0, changed.stderr)
                self.assertEqual(len(self.tags(component)), 4)
                self.assertEqual(self.count.read_text(), str(prior_builds + 2))
                self.assertEqual(len([tag for tag in self.tags(component)
                                      if self.git("rev-list", "-n", "1", tag) == self.git("rev-parse", "HEAD")
                                      and "/v" in tag]), 1)
                # Other components' image inputs did not change.
                for other in inputs:
                    if other != component and self.tags(other):
                        before = self.count.read_text()
                        self.assertEqual(self.run_ci(other).returncode, 0)
                        self.assertEqual(self.count.read_text(), before)

    def test_failure_paths_do_not_create_tags(self):
        self.assertNotEqual(self.run_ci("ingester", FAKE_CHECK_FAIL="1").returncode, 0)
        self.assertNotEqual(self.run_ci("ingester", FAKE_BUILD_FAIL="1").returncode, 0)
        self.assertEqual(self.tags("ingester"), [])
        self.assertEqual(self.run_ci("ingester").returncode, 0)
        tags = self.tags("ingester")
        state = json.loads(self.state.read_text())
        state.pop("capitonic/ingester:v1.2.1-local.0")
        self.state.write_text(json.dumps(state))
        missing = self.run_ci("ingester")
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("missing", missing.stderr)
        self.assertEqual(self.tags("ingester"), tags)

    def test_missing_hash_tag_and_dirty_tree_fail_closed(self):
        (self.repo / "packages/market-data-ingester/src/main.rs").write_text("dirty\n")
        self.assertNotEqual(self.run_ci("ingester").returncode, 0)
        self.assertEqual(self.count.read_text(), "0")
        self.git("checkout", "--", "packages/market-data-ingester/src/main.rs")
        self.assertEqual(self.run_ci("ingester").returncode, 0)
        hash_tag = next(tag for tag in self.tags("ingester") if "/sha256-" in tag)
        self.git("tag", "-d", hash_tag)
        failed = self.run_ci("ingester")
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("lacks mandatory hash tag", failed.stderr)
        self.assertEqual(self.count.read_text(), "1")

    def test_existing_component_lock_blocks_build(self):
        lock = pathlib.Path(self.git("rev-parse", "--path-format=absolute", "--git-common-dir")) / "local-image-ci-ingester.lock"
        lock.mkdir()
        blocked = self.run_ci("ingester")
        self.assertNotEqual(blocked.returncode, 0)
        self.assertIn("refusing concurrent version allocation", blocked.stderr)
        self.assertEqual(self.count.read_text(), "0")


if __name__ == "__main__":
    unittest.main()
