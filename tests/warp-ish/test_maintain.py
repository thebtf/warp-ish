"""Regression tests using isolated synthetic repositories and local-only remotes."""

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "script" / "warp-ish" / "maintain.py"
SPEC = importlib.util.spec_from_file_location("maintain", SCRIPT)
maintain = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(maintain)
POLICY = json.loads(SCRIPT.with_name("terminal-policy.json").read_text(encoding="utf-8"))


class MaintenanceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        template = self.root / "git-template"
        template.mkdir()
        environment = {key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")}
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_TEMPLATE_DIR=str(template))
        environment_patch = mock.patch.dict(os.environ, environment, clear=True)
        environment_patch.start()
        self.addCleanup(environment_patch.stop)
        self.repo = self.root / "source"
        self.repo.mkdir()
        self.git("init", "-b", "warp-ish/main")
        self.hooks = self.repo / ".git/hooks"
        self.hooks.mkdir()
        self.git("config", "core.hooksPath", self.hooks.as_posix())
        self.git("config", "core.autocrlf", "false")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "user.name", "Warp-ish Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        info = self.repo / ".git" / "info"
        info.mkdir(exist_ok=True)
        (info / "exclude").write_text(".work/\n", encoding="utf-8")
        self.fixture_worktrees = {}
        self.write("Cargo.toml", '[workspace]\nmembers = ["app", "crates/warp_terminal"]\n[workspace.dependencies]\nwarp_terminal = { path = "crates/warp_terminal" }\n')
        self.write("app/Cargo.toml", '[package]\nname = "warp"\nversion = "0.0.0"\n[dependencies]\nwarp_terminal.workspace = true\n[features]\ndefault = ["shell_selector"]\nshell_selector = []\nlocal_tty = ["warp_terminal/local_tty"]\n')
        self.write("crates/warp_terminal/Cargo.toml", '[package]\nname = "warp_terminal"\nversion = "0.0.0"\n[features]\nlocal_tty = []\n')
        self.write("app/src/lib.rs", "pub mod terminal;\nfn start() { terminal::init(); }\n")
        self.write("app/src/app_state.rs", "pub struct TerminalPaneSnapshot {\n pub cwd: Option<String>,\n pub is_active: bool,\n}\n")
        self.write("app/src/settings/init.rs", "fn register_all_settings(ctx: &mut Context) { TerminalSettings::register(ctx); }\n")
        self.write("app/src/root_view.rs", "fn start(ctx: &mut Context) { TerminalWorkspace::handle(ctx); }\n")
        self.write("core.txt", "base\n")
        self.base = self.save("fixture base")
        self.policy = copy.deepcopy(POLICY)
        self.policy.pop("reviewed_terminal_drift", None)
        self.policy["observed_upstream_sha"] = self.base
        manifests = {name: tomllib.loads((self.repo / name).read_text()) for name in self.policy["manifests"]}
        workspace = manifests["Cargo.toml"]["workspace"]["dependencies"]
        for name, manifest in manifests.items():
            rows = maintain.dependency_rows(name, manifest, workspace)
            self.policy["observed_files"][name] = {
                "sha256": maintain.digest((self.repo / name).read_bytes()),
                "dependencies": {f'{row["section"]}|{row["alias"]}': {"sha256": maintain.digest(json.dumps(row["spec"], sort_keys=True).encode())} for row in rows},
                "features": sorted(manifest.get("features", {})),
            }
        for name in self.policy["registration_files"]:
            self.policy["observed_files"][name] = {
                "sha256": maintain.digest((self.repo / name).read_bytes()),
                "registrations": maintain.registration_inventory(maintain.rust_code((self.repo / name).read_text())),
            }
        self.policy_path = self.root / "fixture-policy.json"
        self.policy_path.write_text(json.dumps(self.policy), encoding="utf-8")

    def git(self, *args, input_data=None, repo=None):
        result = subprocess.run(["git", "-C", str(repo or self.repo), *args], input=input_data,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        return result.stdout.decode().strip()

    def write(self, name, data):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(data, encoding="utf-8", newline="\n")

    def save(self, message):
        self.git("add", "--all")
        self.git("commit", "-m", message)
        return self.git("rev-parse", "HEAD")

    def audit(self):
        return maintain.audit(self.repo, self.policy, "fixture-policy-hash")

    def candidate_args(self, fork, target, name="WARP-991", *, prepare=True):
        worktree = self.repo / ".work" / "wt" / name
        if prepare:
            worktree.parent.mkdir(parents=True, exist_ok=True)
            self.git("clone", "--shared", "--no-checkout", str(self.repo), str(worktree))
            self.git("switch", "--create", f"task/{name}", target, repo=worktree)
            self.fixture_worktrees[worktree] = {"head": target, "branch": f"refs/heads/task/{name}"}
        return argparse.Namespace(repo=str(self.repo), base=self.base, fork=fork, target=target,
                                  worktree=str(worktree), expected_tree=None)

    def run_candidate(self, args):
        real_git = maintain.git

        def git_with_registration(repo, *command, **options):
            # Real Git validates and replays against local fixture clones. Stand in for
            # the linked-worktree metadata that nvmd-ai supplies in production.
            if command == ("worktree", "list", "--porcelain", "-z"):
                records = [f"worktree {self.repo.as_posix()}\0\0"]
                for path, metadata in self.fixture_worktrees.items():
                    record = f"worktree {path.as_posix()}\0HEAD {metadata['head']}\0branch {metadata['branch']}\0"
                    if metadata.get("state"):
                        record += f"{metadata['state']}\0"
                    records.append(record + "\0")
                return subprocess.CompletedProcess(command, 0, "".join(records).encode(), b"")
            if command == ("rev-parse", "--path-format=absolute", "--git-common-dir"):
                metadata = self.fixture_worktrees.get(repo)
                if metadata is not None and not metadata.get("foreign"):
                    repo = self.repo
            return real_git(repo, *command, **options)

        with mock.patch.object(maintain, "git", side_effect=git_with_registration):
            return maintain.candidate(args, self.policy, "fixture")

    def diverge(self, conflict=False):
        self.write("core.txt" if conflict else "fork.txt", "fork change\n")
        fork = self.save("fork delta")
        self.git("checkout", "-b", "upstream-fixture", self.base)
        self.write("core.txt" if conflict else "upstream.txt", "upstream change\n")
        target = self.save("upstream delta")
        self.git("checkout", "warp-ish/main")
        return fork, target

    def cli(self, *args):
        result = subprocess.run([sys.executable, str(SCRIPT), "--policy", str(self.policy_path), *args],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        return result.returncode, json.loads(result.stdout)

    def test_standalone_agent_tui_source_fails_without_dependency(self):
        self.write("crates/warp_tui/src/lib.rs", "// restored standalone agent frontend\n")
        report = self.audit()
        self.assertEqual(report["checks"]["dependencies"]["findings"], [])
        self.assertIn({"path": "crates/warp_tui", "boundary": "built_in_agents", "status": "present"}, report["checks"]["source_paths"]["findings"])
        self.assertEqual(report["status"], "fail")
        self.assertEqual(report["scoped_removal"]["status"], "fail")
        self.assertEqual(report["terminal_only"]["status"], "fail")

    def test_source_policy_absent_or_missing_is_backward_compatible(self):
        self.assertEqual(self.audit()["checks"]["source_paths"]["status"], "pass")
        self.assertTrue(all(row["status"] == "absent" for row in self.audit()["inventory"]["source_paths"]))
        del self.policy["forbidden_source_paths"]
        report = self.audit()
        self.assertEqual(report["inventory"]["source_paths"], [])
        self.assertEqual(report["scoped_removal"]["status"], "pass")

    def test_source_policy_preserves_shared_toolkit_and_external_omp(self):
        self.write("crates/warpui_core/src/elements/tui/mod.rs", "pub struct SharedCell;\n")
        self.write("app/src/terminal/external_omp.rs", "// child PTY process\n")
        report = self.audit()
        self.assertEqual(report["checks"]["source_paths"]["findings"], [])
        self.assertEqual(report["scoped_removal"]["status"], "pass")

    def test_app_voice_implementation_fails_source_gate_without_dependency_or_registration(self):
        paths = {
            "app/src/voice",
            "app/src/server/voice_transcriber.rs",
            "app/src/editor/view/voice.rs",
            "app/src/settings/tui_voice.rs",
        }
        for relative in paths:
            self.assertEqual(self.policy["forbidden_source_paths"][relative], "cloud_voice")
            self.write(relative + "/mod.rs" if relative == "app/src/voice" else relative,
                       '#[cfg(feature = "retired_voice")]\nfn dormant_voice_implementation() {}\n')
        report = self.audit()
        self.assertEqual(report["checks"]["source_paths"]["status"], "fail")
        self.assertEqual({row["path"] for row in report["checks"]["source_paths"]["findings"]}, paths)

    def test_forbidden_source_file_and_directory_entries_are_both_present(self):
        self.policy["forbidden_source_paths"] = {"retired.rs": "built_in_agents", "retired": "built_in_agents"}
        self.write("retired.rs", "// retained source\n")
        (self.repo / "retired").mkdir()
        self.assertEqual({row["path"] for row in self.audit()["checks"]["source_paths"]["findings"]}, {"retired.rs", "retired"})

    def test_invalid_source_paths_fail_closed_without_outside_probe(self):
        outside = self.root / "outside"
        outside.mkdir()
        for relative in ("../outside", "/absolute", "C:/source", "a/../b", "a//b", "a\\b", ".", "", "a."):
            with self.subTest(path=relative):
                self.policy["forbidden_source_paths"] = {relative: "built_in_agents"}
                with mock.patch.object(Path, "lstat", side_effect=AssertionError("Invalid policy path must not be probed")):
                    _, findings, unknown = maintain.source_path_inventory(self.repo, self.policy)
                self.assertEqual(findings, [])
                self.assertEqual(len(unknown), 1)
                report = self.audit()
                self.assertEqual(report["scoped_removal"]["status"], "unverified")
                self.assertEqual(report["checks"]["source_paths"]["status"], "unverified")
        self.policy["forbidden_source_paths"] = []
        self.assertEqual(self.audit()["scoped_removal"]["status"], "unverified")

    def test_unreadable_source_probe_is_unverified(self):
        self.policy["forbidden_source_paths"] = {"blocked": "built_in_agents"}
        with mock.patch.object(Path, "lstat", side_effect=PermissionError("fixture denied")):
            report = self.audit()
        self.assertEqual(report["checks"]["source_paths"]["status"], "unverified")
        self.assertEqual(report["scoped_removal"]["status"], "unverified")

    def test_source_symlink_entry_counts_but_ancestor_is_not_followed(self):
        link = self.repo / "retired-link"
        try:
            link.symlink_to(self.root / "missing-source", target_is_directory=True)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"Symlink creation unavailable: {error}")
        self.policy["forbidden_source_paths"] = {"retired-link": "built_in_agents"}
        self.assertEqual(self.audit()["checks"]["source_paths"]["status"], "fail")
        self.policy["forbidden_source_paths"] = {"retired-link/child": "built_in_agents"}
        self.assertEqual(self.audit()["scoped_removal"]["status"], "unverified")

    def test_source_only_candidate_prepares_but_removal_gate_fails(self):
        self.write("crates/warp_tui/src/lib.rs", "// standalone frontend\n")
        fork = self.save("restore standalone frontend")
        report = self.run_candidate(self.candidate_args(fork, self.base))
        self.assertEqual(report["preparation"]["status"], "pass")
        self.assertEqual(report["audit"]["checks"]["dependencies"]["findings"], [])
        self.assertEqual(report["audit"]["checks"]["source_paths"]["status"], "fail")
        self.assertEqual(report["status"], "fail")

    def test_source_violation_reaches_cli_exit_status(self):
        self.write("crates/warp_tui/src/lib.rs", "// standalone frontend\n")
        code, report = self.cli("audit", "--repo", str(self.repo))
        self.assertEqual(code, 1)
        self.assertEqual(report["checks"]["source_paths"]["status"], "fail")
        self.policy["forbidden_source_paths"] = {"../outside": "built_in_agents"}
        self.policy_path.write_text(json.dumps(self.policy), encoding="utf-8")
        code, report = self.cli("audit", "--repo", str(self.repo))
        self.assertEqual(code, 2)
        self.assertEqual(report["scoped_removal"]["status"], "unverified")

    def test_empty_scoped_findings_never_claim_terminal_only_pass(self):
        report = self.audit()
        self.assertEqual(report["scoped_removal"]["status"], "pass")
        self.assertEqual(report["terminal_only"]["status"], "unverified")
        self.assertEqual(report["terminal_baseline"]["status"], "unverified")
        self.assertEqual(report["status"], "unverified")

    def test_fixture_has_isolated_git_settings(self):
        self.assertEqual(os.environ["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(os.environ["GIT_CONFIG_GLOBAL"], os.devnull)
        self.assertNotIn("GIT_CONFIG_COUNT", os.environ)
        self.assertNotIn("GIT_CONFIG_PARAMETERS", os.environ)
        self.assertNotIn("GIT_CONFIG_KEY_0", os.environ)
        self.assertNotIn("GIT_CONFIG_VALUE_0", os.environ)
        self.assertEqual(self.git("config", "--get", "core.autocrlf"), "false")
        self.assertEqual(self.git("config", "--get", "commit.gpgsign"), "false")
        self.assertEqual(self.git("config", "--get", "core.hooksPath"), self.hooks.as_posix())
        self.assertEqual(list(self.hooks.iterdir()), [])

    def test_fixture_ignores_external_global_hooks_and_config_injection(self):
        external_hooks = self.root / "external-hooks"
        external_hooks.mkdir()
        marker = self.root / "global-hook-fired.txt"
        hook = external_hooks / "pre-commit"
        hook.write_text(f"#!/bin/sh\nprintf 'hook fired\\n' >> {shlex.quote(marker.as_posix())}\n",
                        encoding="utf-8", newline="\n")
        hook.chmod(0o755)
        self.git("-c", f"core.hooksPath={external_hooks.as_posix()}", "commit", "--allow-empty", "-m", "hook positive control")
        self.assertEqual(marker.read_text(), "hook fired\n")
        marker.unlink()
        config = self.root / "external-global.gitconfig"
        self.git("config", "--file", str(config), "core.hooksPath", external_hooks.as_posix())
        self.git("config", "--file", str(config), "commit.gpgsign", "true")
        config_before = config.read_bytes()
        environment = dict(os.environ, GIT_CONFIG_NOSYSTEM="0", GIT_CONFIG_GLOBAL=str(config),
                           GIT_CONFIG_SYSTEM=str(config), GIT_CONFIG_COUNT="2",
                           GIT_CONFIG_KEY_0="core.hooksPath", GIT_CONFIG_VALUE_0=external_hooks.as_posix(),
                           GIT_CONFIG_KEY_1="commit.gpgsign", GIT_CONFIG_VALUE_1="true",
                           GIT_CONFIG_PARAMETERS="malformed inherited parameters")
        result = subprocess.run([sys.executable, "-m", "unittest", "test_maintain.MaintenanceTests.test_fixture_has_isolated_git_settings", "-v"],
                                cwd=ROOT / "tests/warp-ish", env=environment,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertFalse(marker.exists())
        self.assertEqual(config.read_bytes(), config_before)

    def test_audit_does_not_modify_checkout_or_index(self):
        index_before = (self.repo / ".git/index").read_bytes()
        before = maintain.checkout_state(self.repo)
        self.audit()
        self.assertEqual(before, maintain.checkout_state(self.repo))
        self.assertEqual(index_before, (self.repo / ".git/index").read_bytes())

    def test_agent_modules_and_init_calls_fail_with_line_evidence(self):
        self.write("app/src/lib.rs", "// ordinary comment\nmod ai;\nfn start() { ai::init(ctx); }\n")
        report = self.audit()
        findings = report["checks"]["registrations"]["findings"]
        self.assertEqual(report["status"], "fail")
        self.assertEqual([row["line"] for row in findings], [2, 3])
        self.assertEqual(report["terminal_baseline"]["status"], "unverified")

    def test_comments_strings_raw_strings_and_char_literals_are_masked(self):
        self.write("app/src/lib.rs", '''pub mod terminal;
// mod ai;
/* outer /* auth::init(ctx); */ mod drive; */
const EXAMPLE: &str = "mod ai; auth::init(ctx);";
const RAW: &str = r##"mod billing; \" drive::init(ctx);"##;
const BYTES: &[u8] = br#"mod pricing;"#;
fn sample<'a>(s: &'a str) { let quote = '\\''; let slash = '/'; }
fn start() { terminal::init(); }
''')
        self.assertEqual(self.audit()["checks"]["registrations"]["findings"], [])

    def test_unterminated_source_is_unverified(self):
        self.write("app/src/lib.rs", 'const SOURCE: &str = r##"unterminated')
        report = self.audit()
        self.assertEqual(report["scoped_removal"]["status"], "unverified")
        self.assertIn("unparsed_source", {row["kind"] for row in report["checks"]["scope_and_drift"]["findings"]})

    def test_forbidden_snapshot_fields_fail(self):
        self.write("app/src/app_state.rs", "pub struct TerminalPaneSnapshot {\n pub conversation_ids_to_restore: Vec<Id>,\n}\n")
        self.assertEqual(self.audit()["checks"]["registrations"]["status"], "fail")

    def test_persisted_agent_members_fail_with_any_visibility(self):
        for visibility in ("pub ", "pub(crate) ", ""):
            with self.subTest(visibility=visibility):
                self.write("app/src/app_state.rs", f"{visibility}struct AmbientAgentPaneSnapshot {{\n {visibility}llm_model_override: String,\n}}\n")
                findings = self.audit()["checks"]["registrations"]["findings"]
                self.assertEqual(len(findings), 2)

    def test_raw_rust_identifiers_do_not_hide_known_registration(self):
        self.write("app/src/lib.rs", "mod r#ai;\nfn start() { r#ai::r#init(ctx); }\n")
        self.write("app/src/app_state.rs", "struct r#AmbientAgentPaneSnapshot { r#llm_model_override: String, }\n")
        self.assertEqual(len(self.audit()["checks"]["registrations"]["findings"]), 4)

    def test_crlf_only_changes_byte_hashes_without_semantic_drift(self):
        for name in self.policy["observed_files"]:
            path = self.repo / name
            path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
        report = self.audit()
        self.assertTrue(all(record["changed_from_observed"] for record in report["source"]["files"].values()))
        self.assertEqual(report["checks"]["scope_and_drift"]["findings"], [])
        self.assertEqual(report["scoped_removal"]["status"], "pass")

    def test_settings_schema_and_root_auth_services_fail(self):
        self.write("app/src/settings/init.rs", "fn register(ctx: &mut Context) { AISettings::register_and_subscribe_to_events(ctx); CloudPreferencesSettings::register(ctx); }\n")
        self.write("app/src/root_view.rs", "fn start(ctx: &mut Context) { AuthManager::handle(ctx); AuthView::new(AuthViewVariant::Initial, ctx); }\n")
        report = self.audit()
        findings = report["checks"]["registrations"]["findings"]
        self.assertEqual(len(findings), 4)
        self.assertEqual(report["status"], "fail")

    def test_new_service_factory_requires_review(self):
        self.write("app/src/root_view.rs", "fn start(ctx: &mut Context) { FutureService::handle(ctx); }\n")
        report = self.audit()
        self.assertEqual(report["scoped_removal"]["status"], "unverified")
        self.assertIn("new_service_calls", {row["kind"] for row in report["checks"]["scope_and_drift"]["findings"]})

    def test_reviewed_dependency_requires_the_exact_spec(self):
        path = "app/Cargo.toml"
        manifest = (self.repo / path).read_text(encoding="utf-8")
        key = "dependencies|terminal_ui"
        review = {"sha256": maintain.digest(json.dumps({"version": "1.0"}, sort_keys=True).encode())}
        self.policy["reviewed_terminal_drift"] = {"dependencies": {path: {key: review}}}
        for version, expected in (("1.0", []), ("2.0", [key])):
            with self.subTest(version=version):
                self.write(path, manifest.replace("[dependencies]\n", f'[dependencies]\nterminal_ui = "{version}"\n'))
                findings = self.audit()["checks"]["scope_and_drift"]["findings"]
                self.assertEqual([row["dependency"] for row in findings], expected)

    def test_reviewed_registration_cannot_suppress_a_forbidden_rule(self):
        path = "app/src/settings/init.rs"
        name = "AISettings::register_and_subscribe_to_events"
        self.policy["reviewed_terminal_drift"] = {"registrations": {path: {"service_calls": {name: "fixture"}}}}
        self.write(path, f"fn register(ctx: &mut Context) {{ {name}(ctx); }}\n")
        checks = self.audit()["checks"]
        self.assertEqual(checks["scope_and_drift"]["findings"], [])
        self.assertEqual(len(checks["registrations"]["findings"]), 1)

    def test_optional_dependency_alias_cannot_hide_forbidden_package(self):
        self.write("app/Cargo.toml", '[package]\nname="warp"\nversion="0.0.0"\n[dependencies]\nassistant={package="ai",version="1",optional=true}\n[features]\ndefault=[]\n')
        report = self.audit()
        row = report["checks"]["dependencies"]["findings"][0]
        self.assertEqual(row["package"], "ai")
        self.assertTrue(row["optional"])
        self.assertEqual(report["status"], "fail")

    def test_path_dependency_alias_is_checked_against_known_subsystem_path(self):
        self.write("app/Cargo.toml", '[package]\nname="warp"\nversion="0.0.0"\n[dependencies]\nassistant={path="../crates/ai"}\n')
        self.assertEqual(self.audit()["checks"]["dependencies"]["status"], "fail")

    def test_agent_tui_policy_keeps_shared_tui_toolkit_available(self):
        for dependency in (
            'warp_tui="1"',
            'frontend={package="warp_tui",version="1",optional=true}',
            'frontend={path="../crates/warp_tui"}',
        ):
            with self.subTest(dependency=dependency):
                self.write("app/Cargo.toml", '[package]\nname="warp"\nversion="0.0.0"\n[dependencies]\n' + dependency + '\n')
                rows = self.audit()["checks"]["dependencies"]["findings"]
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]["boundaries"], {"warp_tui": "built_in_agents"})
        self.write("app/Cargo.toml", '[package]\nname="warp"\nversion="0.0.0"\n[dependencies]\nwarpui_core={path="../crates/warpui_core",features=["tui"]}\n[features]\ntui=["warpui_core/tui"]\n')
        report = self.audit()
        self.assertEqual(report["checks"]["dependencies"]["findings"], [])
        self.assertEqual(report["checks"]["features"]["findings"], [])

    def test_target_and_dev_dependency_seeds_are_recorded(self):
        self.write("app/Cargo.toml", '[package]\nname="warp"\nversion="0.0.0"\n[dev-dependencies]\nai="1"\n[target.\'cfg(windows)\'.dependencies]\nmcp="1"\n')
        rows = self.audit()["checks"]["dependencies"]["findings"]
        self.assertEqual({row["section"] for row in rows}, {"dev-dependencies", "target.cfg(windows).dependencies"})

    def test_workspace_features_are_additive(self):
        manifest = {"dependencies": {"shared": {"workspace": True, "features": ["child"]}}}
        workspace = {"shared": {"package": "ai", "version": "1", "features": ["parent"]}}
        row = maintain.dependency_rows("app/Cargo.toml", manifest, workspace)[0]
        self.assertEqual(row["spec"]["features"], ["parent", "child"])
        self.assertEqual(row["package"], "ai")

    def test_nondefault_forbidden_feature_still_fails(self):
        path = self.repo / "app/Cargo.toml"
        path.write_text(path.read_text() + "agent_mode=[]\n", encoding="utf-8")
        report = self.audit()
        self.assertEqual(report["inventory"]["features"]["app/Cargo.toml"]["default"], ["shell_selector"])
        self.assertEqual(report["checks"]["features"]["status"], "fail")

    def test_forbidden_feature_edge_fails_without_forbidden_definition(self):
        path = self.repo / "app/Cargo.toml"
        path.write_text(path.read_text() + 'gui=["dep:voice_input"]\n', encoding="utf-8")
        self.assertEqual(self.audit()["checks"]["features"]["status"], "fail")

    def test_new_service_feature_and_changed_dependency_are_unknown(self):
        self.write("app/src/lib.rs", "pub mod terminal;\nmod future_service;\nfn start() { future_service::init(); }\n")
        path = self.repo / "app/Cargo.toml"
        path.write_text(path.read_text().replace("warp_terminal.workspace = true", 'warp_terminal={workspace=true,features=["future"]}') + "future=[]\n", encoding="utf-8")
        report = self.audit()
        kinds = {row["kind"] for row in report["checks"]["scope_and_drift"]["findings"]}
        self.assertTrue({"new_modules", "new_init_calls", "new_feature", "new_or_changed_dependency"} <= kinds)
        self.assertEqual(report["scoped_removal"]["status"], "unverified")

    def test_missing_manifest_is_unverified(self):
        (self.repo / "crates/warp_terminal/Cargo.toml").unlink()
        self.assertEqual(self.audit()["scoped_removal"]["status"], "unverified")

    def test_invalid_toml_is_unverified(self):
        self.write("app/Cargo.toml", "[bad\n")
        self.assertEqual(self.audit()["scoped_removal"]["status"], "unverified")

    def test_concurrent_source_change_is_unverified(self):
        original = maintain.checkout_state
        calls = 0
        def change_during_read(repo):
            nonlocal calls
            calls += 1
            if calls == 2:
                self.write("app/src/lib.rs", "pub mod terminal;\n// concurrent edit\n")
            return original(repo)
        with mock.patch.object(maintain, "checkout_state", side_effect=change_during_read):
            report = self.audit()
        self.assertIn("checkout_changed_during_audit", {row["kind"] for row in report["checks"]["scope_and_drift"]["findings"]})

    def test_candidate_replay_keeps_source_clean_and_is_reproducible(self):
        fork, target = self.diverge()
        before = maintain.checkout_state(self.repo)
        index_before = (self.repo / ".git/index").read_bytes()
        args = self.candidate_args(fork, target)
        report = self.run_candidate(args)
        self.assertEqual(report["preparation"]["status"], "pass", report)
        self.assertEqual(report["main_guard"]["status"], "pass")
        self.assertEqual(before, maintain.checkout_state(self.repo))
        self.assertEqual(index_before, (self.repo / ".git/index").read_bytes())
        candidate = Path(args.worktree)
        self.assertEqual(self.git("rev-parse", "HEAD", repo=candidate), target)
        self.assertEqual(self.git("branch", "--show-current", repo=candidate), "task/WARP-991")
        self.assertEqual((candidate / "fork.txt").read_text(), "fork change\n")
        self.assertTrue((candidate / "upstream.txt").is_file())
        self.assertEqual(self.git("diff", "--cached", "--name-only", repo=candidate), "fork.txt")
        second_args = self.candidate_args(fork, target, "WARP-992")
        second_args.expected_tree = report["pins"]["candidate_tree"]
        second = self.run_candidate(second_args)
        self.assertEqual(second["preparation"]["status"], "pass", second)
        self.assertEqual(second["pins"], report["pins"])

    def test_conflict_preserves_candidate_and_original_source(self):
        fork, target = self.diverge(conflict=True)
        args = self.candidate_args(fork, target)
        report = self.run_candidate(args)
        self.assertEqual(report["preparation"]["status"], "fail")
        self.assertEqual(report["conflicts"], ["core.txt"])
        self.assertTrue(Path(args.worktree).is_dir())
        self.assertEqual((self.repo / "core.txt").read_text(), "fork change\n")
        self.assertTrue(maintain.checkout_state(self.repo)["clean"])

    def test_candidate_never_checks_out_again_or_changes_hook_configuration(self):
        fork, target = self.diverge()
        args = self.candidate_args(fork, target)
        destination = Path(args.worktree)
        hooks = self.root / "candidate-hooks"
        hooks.mkdir()
        marker = self.root / "unexpected-checkout"
        hook = hooks / "post-checkout"
        hook.write_text(f"#!/bin/sh\nprintf 'hook fired\\n' >> {shlex.quote(marker.as_posix())}\n",
                        encoding="utf-8", newline="\n")
        hook.chmod(0o755)
        self.git("config", "core.hooksPath", hooks.as_posix(), repo=destination)

        run = self.run_candidate(args)

        self.assertEqual(run["preparation"]["status"], "pass", run)
        self.assertFalse(marker.exists())
        self.assertEqual(self.git("config", "--get", "core.hooksPath", repo=destination), hooks.as_posix())
        self.assertEqual(self.git("config", "--get", "core.hooksPath"), self.hooks.as_posix())

    def test_candidate_rejects_stale_head_sha(self):
        fork, target = self.diverge()
        args = self.candidate_args(self.base, target)
        report = self.run_candidate(args)
        self.assertEqual(report["preparation"]["status"], "fail")
        self.assertTrue(maintain.checkout_state(Path(args.worktree))["clean"])
        self.assertIn("Source HEAD", report["error"])
        self.assertEqual(self.git("rev-parse", "HEAD"), fork)

    def test_candidate_rejects_short_or_mutable_commit_refs(self):
        for value in (self.base[:8], "warp-ish/main", "HEAD"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                maintain.commit(self.repo, value)

    def test_candidate_rejects_non_ancestor_target(self):
        tree = self.git("rev-parse", "HEAD^{tree}")
        unrelated = self.git("commit-tree", tree, input_data=b"unrelated root\n")
        report = self.run_candidate(self.candidate_args(self.base, unrelated, prepare=False))
        self.assertEqual(report["preparation"]["status"], "fail")
        self.assertIn("ancestor", report["error"])

    def test_candidate_rejects_dirty_source_without_changing_it(self):
        self.write("untracked.txt", "keep me\n")
        args = self.candidate_args(self.base, self.base)
        before = maintain.checkout_state(self.repo)
        report = self.run_candidate(args)
        self.assertEqual(report["preparation"]["status"], "fail")
        self.assertEqual(before, maintain.checkout_state(self.repo))
        self.assertEqual((self.repo / "untracked.txt").read_text(), "keep me\n")
        self.assertTrue(maintain.checkout_state(Path(args.worktree))["clean"])

    def test_candidate_rejects_missing_source_and_unregistered_destinations(self):
        fork, target = self.diverge()
        args = self.candidate_args(fork, target, prepare=False)
        missing = args.worktree
        self.assertIn("separate existing", self.run_candidate(args)["error"])
        self.assertFalse(Path(missing).exists())

        args.worktree = str(self.repo)
        self.assertIn("separate existing", self.run_candidate(args)["error"])

        args = self.candidate_args(fork, target)
        destination = Path(args.worktree)
        self.fixture_worktrees.pop(destination)
        self.assertIn("registered", self.run_candidate(args)["error"])
        self.assertTrue(maintain.checkout_state(destination)["clean"])

    def test_candidate_rejects_wrong_branch_head_lock_and_git_directory(self):
        fork, target = self.diverge()
        args = self.candidate_args(fork, target)
        destination = Path(args.worktree)
        registration = self.fixture_worktrees[destination]
        baseline = dict(registration)
        for change, error in [
            ({"branch": "refs/heads/other"}, "task branch"),
            ({"head": self.base}, "exact --target SHA"),
            ({"state": "locked for another operation"}, "active, unlocked"),
            ({"state": "prunable stale entry"}, "active, unlocked"),
            ({"foreign": True}, "share the source repository"),
        ]:
            with self.subTest(change=change):
                registration.clear()
                registration.update(baseline | change)
                self.assertIn(error, self.run_candidate(args)["error"])
                self.assertTrue(maintain.checkout_state(destination)["clean"])
                self.assertFalse((destination / "fork.txt").exists())

    def test_candidate_rejects_dirty_destination_without_changing_it(self):
        fork, target = self.diverge()
        args = self.candidate_args(fork, target)
        destination = Path(args.worktree)
        unrelated = destination / "keep.txt"
        unrelated.write_text("keep me", encoding="utf-8")
        before = maintain.checkout_state(destination)
        self.assertIn("clean before replay", self.run_candidate(args)["error"])
        self.assertEqual(before, maintain.checkout_state(destination))
        self.assertEqual(unrelated.read_text(encoding="utf-8"), "keep me")

    def test_expected_tree_mismatch_fails(self):
        fork, target = self.diverge()
        args = self.candidate_args(fork, target)
        args.expected_tree = "0" * 40
        report = self.run_candidate(args)
        self.assertEqual(report["preparation"]["status"], "fail")
        self.assertIn("expected-tree", report["error"])

    def test_agentful_candidate_can_prepare_but_removal_gate_fails(self):
        self.write("app/src/lib.rs", "mod ai;\n")
        fork = self.save("keep full agent subsystem")
        report = self.run_candidate(self.candidate_args(fork, self.base))
        self.assertEqual(report["preparation"]["status"], "pass")
        self.assertEqual(report["audit"]["terminal_only"]["status"], "fail")
        self.assertEqual(report["status"], "fail")

    def observer_args(self, state=None):
        return argparse.Namespace(repo=str(self.repo), base=self.base, remote="upstream", timeout=5, state=str(state) if state else None)

    def test_observer_parses_annotated_and_unknown_tags_without_inferring_ancestry(self):
        object_sha = "1" * 40
        other_sha = "2" * 40
        output = f"ref: refs/heads/master\tHEAD\n{other_sha}\tHEAD\n{other_sha}\trefs/heads/master\n{object_sha}\trefs/tags/v1.2.3\n{other_sha}\trefs/tags/v1.2.3^{{}}\n{other_sha}\trefs/tags/v1.2.4-beta\n{other_sha}\trefs/tags/v0.2026.11.06.08.12.stable_00\n"
        report = maintain.remote_observation(output, self.base, POLICY["stable_tag_pattern"])
        self.assertEqual(len(report["stable_tags"]), 2)
        annotated = next(tag for tag in report["stable_tags"] if tag["name"] == "v1.2.3")
        self.assertEqual(annotated["target_sha"], other_sha)
        self.assertEqual(annotated["object_sha"], object_sha)
        self.assertFalse(report["comparison"]["head_equal_to_base"])
        self.assertEqual(report["comparison"]["ancestry"], "unverified")
        self.assertEqual(len(report["tags"]), 3)

    def test_observer_reads_local_remote_without_mutating_refs(self):
        self.git("remote", "add", "upstream", str(self.repo))
        refs_before = self.git("show-ref")
        state = self.root / "evidence" / "last-good.json"
        report = maintain.observe_upstream(self.observer_args(state), self.policy)
        self.assertEqual(report["status"], "pass")
        self.assertEqual(self.git("show-ref"), refs_before)
        self.assertTrue(report["comparison"]["head_equal_to_base"])
        self.assertEqual(json.loads(state.read_text())["head"]["sha"], self.base)
        self.assertEqual(list(state.parent.glob("*.tmp")), [])

    def test_failed_remote_does_not_replace_last_good_state(self):
        self.git("remote", "add", "upstream", str(self.repo))
        state = self.root / "last-good.json"
        maintain.observe_upstream(self.observer_args(state), self.policy)
        previous = state.read_bytes()
        self.git("remote", "set-url", "upstream", str(self.root / "missing-repo"))
        exit_code, report = self.cli("observe-upstream", "--repo", str(self.repo), "--base", self.base, "--state", str(state))
        self.assertEqual(exit_code, 2)
        self.assertEqual(report["status"], "unverified")
        self.assertIn("error", report)
        self.assertEqual(state.read_bytes(), previous)

    def test_remote_timeout_does_not_replace_last_good_state(self):
        self.git("remote", "add", "upstream", str(self.repo))
        state = self.root / "last-good.json"
        maintain.observe_upstream(self.observer_args(state), self.policy)
        previous = state.read_bytes()
        original = maintain.git
        def timeout(repo, *args, **kwargs):
            if args[0] == "ls-remote":
                raise subprocess.TimeoutExpired("git ls-remote", 5)
            return original(repo, *args, **kwargs)
        with mock.patch.object(maintain, "git", side_effect=timeout), self.assertRaises(subprocess.TimeoutExpired):
            maintain.observe_upstream(self.observer_args(state), self.policy)
        self.assertEqual(state.read_bytes(), previous)

    def test_observer_refuses_overwriting_tracked_source(self):
        self.git("remote", "add", "upstream", str(self.repo))
        contents = (self.repo / "Cargo.toml").read_bytes()
        with self.assertRaises(ValueError):
            maintain.observe_upstream(self.observer_args(self.repo / "Cargo.toml"), self.policy)
        self.assertEqual((self.repo / "Cargo.toml").read_bytes(), contents)

    def test_cli_exit_status_distinguishes_fail_and_unverified(self):
        code, report = self.cli("audit", "--repo", str(self.repo))
        self.assertEqual((code, report["status"]), (2, "unverified"))
        self.write("app/src/lib.rs", "mod ai;\n")
        code, report = self.cli("audit", "--repo", str(self.repo))
        self.assertEqual((code, report["status"]), (1, "fail"))

    def test_cli_emits_valid_json_for_unicode_path_with_ascii_stdout(self):
        renamed = self.root / "исходники"
        self.repo.rename(renamed)
        self.repo = renamed
        result = subprocess.run([sys.executable, str(SCRIPT), "--policy", str(self.policy_path), "audit", "--repo", str(self.repo)],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                env=dict(os.environ, PYTHONIOENCODING="ascii"), check=False)
        report = json.loads(result.stdout)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(report["source"]["repo"], str(self.repo))


if __name__ == "__main__":
    unittest.main()
