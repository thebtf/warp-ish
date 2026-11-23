#!/usr/bin/env python3
"""Local, pinned-source maintenance for warp-ish. Python 3.11+, Git, no pip packages."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import tomllib
from datetime import datetime, timezone


POLICY_PATH = Path(__file__).with_name("terminal-policy.json")
SHA_PATTERN = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
MODULE_PATTERN = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*[;{]")
INIT_PATTERN = re.compile(r"\b((?:[A-Za-z_]\w*\s*::\s*)+init)\s*\(")
FIELD_PATTERN = re.compile(r"\bpub(?:\([^)]*\))?\s+(\w+)\s*:")
SERVICE_PATTERN = re.compile(r"\b((?:[A-Za-z_]\w*\s*::\s*)+(?:new|handle|register|register_and_subscribe_to_events))\s*\(")
RAW_STRING_PATTERN = re.compile(r'(?:br|cr|r)(#*)"')
CHAR_PATTERN = re.compile(r"'(?:[^'\\\n]|\\(?:[nrt0\\'\"]|x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}))'")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(repo: Path, *args: str, input_data: bytes | None = None,
        timeout: int = 60, check: bool = True) -> subprocess.CompletedProcess:
    env = dict(os.environ, GIT_OPTIONAL_LOCKS="0", GIT_TERMINAL_PROMPT="0",
               GIT_LFS_SKIP_SMUDGE="1", GCM_INTERACTIVE="never")
    env.pop("GIT_ASKPASS", None)
    env.pop("SSH_ASKPASS", None)
    result = subprocess.run(
        ["git", "-c", "core.askPass=", "-c", "credential.interactive=false", "-C", str(repo), *args], input=input_data,
        stdin=subprocess.DEVNULL if input_data is None else None,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
        timeout=timeout, check=False,
    )
    if check and result.returncode:
        detail = result.stderr.decode("utf-8", errors="replace")[-4000:].strip()
        detail = re.sub(r"(https?://)[^/@\s]+@", r"\1[redacted]@", detail)
        raise RuntimeError(f"git {args[0]} exited {result.returncode}: {detail}")
    return result


def repo_root(path: str | Path) -> Path:
    return Path(git(Path(path), "rev-parse", "--show-toplevel").stdout.decode().strip()).resolve()


def commit(repo: Path, value: str) -> str:
    if not SHA_PATTERN.fullmatch(value):
        raise ValueError("Use a full lowercase immutable commit SHA, not a branch or short SHA")
    resolved = git(repo, "rev-parse", "--verify", f"{value}^{{commit}}").stdout.decode().strip()
    if resolved != value:
        raise ValueError("The supplied SHA is not the commit itself")
    return resolved


def checkout_state(repo: Path) -> dict:
    status = git(repo, "status", "--porcelain=v1", "-z", "--untracked-files=normal").stdout
    return {
        "head": git(repo, "rev-parse", "HEAD").stdout.decode().strip(),
        "branch": git(repo, "symbolic-ref", "--quiet", "--short", "HEAD", check=False).stdout.decode().strip() or None,
        "clean": not status,
        "status_sha256": digest(status),
    }


def rust_code(source: str) -> str:
    """Mask comments and literals and normalize raw identifiers, preserving line numbers."""
    masked = list(source)
    index = 0
    while index < len(source):
        start = index
        if source.startswith("//", index):
            end = source.find("\n", index)
            index = len(source) if end == -1 else end
        elif source.startswith("/*", index):
            depth = 1
            index += 2
            while depth and index < len(source):
                if source.startswith("/*", index):
                    depth += 1
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                raise ValueError("Unterminated Rust block comment")
        else:
            raw = RAW_STRING_PATTERN.match(source, index)
            if raw and (index == 0 or not (source[index - 1].isalnum() or source[index - 1] == "_")):
                terminator = '"' + raw.group(1)
                end = source.find(terminator, raw.end())
                if end == -1:
                    raise ValueError("Unterminated Rust raw string")
                index = end + len(terminator)
            elif source[index] == '"':
                index += 1
                while index < len(source) and source[index] != '"':
                    index += 2 if source[index] == "\\" else 1
                if index >= len(source):
                    raise ValueError("Unterminated Rust string")
                index += 1
            elif source[index] == "'":
                char = CHAR_PATTERN.match(source, index)
                if not char:
                    index += 1  # Rust lifetimes are not character literals.
                    continue
                index = char.end()
            else:
                index += 1
                continue
        for position in range(start, index):
            if masked[position] != "\n":
                masked[position] = " "
    return re.sub(r"\br#(?=[A-Za-z_]\w*)", "", "".join(masked))


def dependency_rows(path: str, manifest: dict, workspace: dict) -> list[dict]:
    sections = []
    for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
        if kind in manifest:
            sections.append((kind, manifest[kind]))
    if "dependencies" in manifest.get("workspace", {}):
        sections.append(("workspace.dependencies", manifest["workspace"]["dependencies"]))
    for target, config in manifest.get("target", {}).items():
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            if kind in config:
                sections.append((f"target.{target}.{kind}", config[kind]))
    rows = []
    for section, entries in sections:
        for alias, raw_spec in entries.items():
            spec = {"version": raw_spec} if isinstance(raw_spec, str) else dict(raw_spec)
            inherited = bool(spec.pop("workspace", False))
            if inherited:
                if alias not in workspace:
                    raise ValueError(f"Unresolved workspace dependency {path}: {alias}")
                parent = workspace[alias]
                parent = {"version": parent} if isinstance(parent, str) else dict(parent)
                features = list(dict.fromkeys(parent.get("features", []) + spec.get("features", [])))
                parent.update(spec)
                if features:
                    parent["features"] = features
                spec = parent
            rows.append({
                "manifest": path, "section": section, "alias": alias,
                "package": spec.get("package", alias), "inherited": inherited,
                "optional": bool(spec.get("optional", False)), "spec": spec,
            })
    return sorted(rows, key=lambda row: (row["section"], row["alias"]))


def registration_inventory(code: str) -> dict:
    return {
        "modules": sorted(set(MODULE_PATTERN.findall(code))),
        "init_calls": sorted({re.sub(r"\s+", "", name) for name in INIT_PATTERN.findall(code)}),
        "public_fields": sorted(set(FIELD_PATTERN.findall(code))),
        "service_calls": sorted({re.sub(r"\s+", "", name) for name in SERVICE_PATTERN.findall(code)}),
    }


def source_path_inventory(repo: Path, policy: dict) -> tuple[list, list, list]:
    """Probe only explicit repo-relative entries, without following source links."""
    configured = policy.get("forbidden_source_paths", {})
    inventory, findings, unknown = [], [], []
    if not isinstance(configured, dict):
        return inventory, findings, [{"kind": "invalid_source_path_policy", "detail": "Expected a path-to-boundary object"}]
    for relative, boundary in configured.items():
        try:
            if (not isinstance(relative, str) or not relative
                    or any(character in relative for character in ("\\", ":"))
                    or any(ord(character) < 32 for character in relative)
                    or any(part in ("", ".", "..") or part.endswith((".", " ")) for part in relative.split("/"))
                    or not isinstance(boundary, str) or not boundary.strip()):
                raise ValueError("Expected a nonempty forward-slash relative path and boundary, without traversal or Windows aliases")
            path = repo
            parts = relative.split("/")
            for index, part in enumerate(parts):
                path = path / part
                try:
                    entry = path.lstat()
                except FileNotFoundError:
                    inventory.append({"path": relative, "boundary": boundary, "status": "absent"})
                    break
                reparse = bool(getattr(entry, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
                if index < len(parts) - 1:
                    if stat.S_ISLNK(entry.st_mode) or reparse:
                        raise ValueError("Source path has a symlink or reparse-point ancestor; outside traversal is not probed")
                    if not stat.S_ISDIR(entry.st_mode):
                        inventory.append({"path": relative, "boundary": boundary, "status": "absent"})
                        break
                else:
                    row = {"path": relative, "boundary": boundary, "status": "present"}
                    inventory.append(row)
                    findings.append(row.copy())
        except (ValueError, OSError) as error:
            row = {"kind": "unverified_source_path", "path": relative, "detail": str(error)}
            unknown.append(row)
            inventory.append({"path": relative, "boundary": boundary, "status": "unverified"})
    return inventory, findings, unknown


def audit(repo: Path, policy: dict, policy_hash: str) -> dict:
    before = checkout_state(repo)
    sources, source_findings, source_unknown = source_path_inventory(repo, policy)
    inventory = {"dependencies": [], "features": {}, "registrations": {}, "source_paths": sources}
    violations = {"dependencies": [], "features": [], "registrations": [], "source_paths": source_findings}
    unknown = list(source_unknown)
    files = {}
    contents = {}
    reviewed = policy.get("reviewed_terminal_drift", {})
    for relative, observed in policy["observed_files"].items():
        path = repo / relative
        if not path.resolve().is_relative_to(repo) or not path.is_file():
            unknown.append({"kind": "missing_or_external_file", "path": relative})
            continue
        raw = path.read_bytes()
        files[relative] = {"sha256": digest(raw), "changed_from_observed": digest(raw) != observed["sha256"]}
        try:
            contents[relative] = raw.decode("utf-8")
        except UnicodeDecodeError:
            unknown.append({"kind": "invalid_utf8", "path": relative})

    manifests = {}
    for relative in policy["manifests"]:
        if relative not in contents:
            continue
        try:
            manifests[relative] = tomllib.loads(contents[relative])
        except tomllib.TOMLDecodeError as error:
            unknown.append({"kind": "invalid_manifest", "path": relative, "detail": str(error)})
    workspace = manifests.get("Cargo.toml", {}).get("workspace", {}).get("dependencies", {})
    forbidden = policy["forbidden_packages"]
    for relative, manifest in manifests.items():
        try:
            rows = dependency_rows(relative, manifest, workspace)
        except ValueError as error:
            unknown.append({"kind": "unresolved_dependencies", "path": relative, "detail": str(error)})
            rows = []
        inventory["dependencies"].extend(rows)
        observed = policy["observed_files"][relative]
        reviewed_dependencies = reviewed.get("dependencies", {}).get(relative, {})
        for row in rows:
            key = f'{row["section"]}|{row["alias"]}'
            spec_hash = digest(json.dumps(row["spec"], sort_keys=True).encode())
            observed_hash = observed["dependencies"].get(key, {}).get("sha256")
            reviewed_hash = reviewed_dependencies.get(key, {}).get("sha256")
            if spec_hash not in (observed_hash, reviewed_hash):
                unknown.append({"kind": "new_or_changed_dependency", "path": relative, "dependency": key})
            names = {row["alias"], row["package"]}
            # Inherited paths are relative to the workspace manifest.
            if "path" in row["spec"]:
                base = repo if row["inherited"] or row["section"] == "workspace.dependencies" else (repo / relative).parent
                dependency_path = (base / row["spec"]["path"]).resolve()
                for package, forbidden_path in policy["forbidden_paths"].items():
                    if dependency_path == (repo / forbidden_path).resolve():
                        names.add(package)
            matched = sorted(names & forbidden.keys())
            if matched:
                violations["dependencies"].append({**row, "boundaries": {name: forbidden[name] for name in matched}})
        features = manifest.get("features", {})
        inventory["features"][relative] = {"default": features.get("default", []), "definitions": features}
        for name, edges in features.items():
            if name not in observed["features"]:
                unknown.append({"kind": "new_feature", "path": relative, "feature": name})
            if name in policy["forbidden_features"]:
                violations["features"].append({"path": relative, "feature": name, "reason": "retained_feature_definition"})
            for edge in edges:
                dependency = edge.removeprefix("dep:").split("/")[0].rstrip("?")
                if dependency in forbidden or edge in policy["forbidden_features"]:
                    violations["features"].append({"path": relative, "feature": name, "edge": edge, "reason": "retained_feature_edge"})

    for relative in policy["registration_files"]:
        if relative not in contents:
            continue
        try:
            code = rust_code(contents[relative])
        except ValueError as error:
            unknown.append({"kind": "unparsed_source", "path": relative, "detail": str(error)})
            continue
        registrations = registration_inventory(code)
        inventory["registrations"][relative] = registrations
        observed = policy["observed_files"][relative]["registrations"]
        reviewed_registrations = reviewed.get("registrations", {}).get(relative, {})
        for kind, names in registrations.items():
            for name in sorted(set(names) - set(observed[kind]) - set(reviewed_registrations.get(kind, {}))):
                unknown.append({"kind": f"new_{kind}", "path": relative, "name": name})
        for rule in policy["source_rules"]:
            if rule["path"] != relative:
                continue
            for match in re.finditer(rule["pattern"], code, re.MULTILINE):
                violations["registrations"].append({
                    "id": rule["id"], "boundary": rule["boundary"], "path": relative,
                    "line": code.count("\n", 0, match.start()) + 1,
                    "match": match.group().strip(),
                })

    # A concurrent edit must not be reported as a coherent, pinned observation.
    after = checkout_state(repo)
    changed_during_read = before != after or any(
        not (repo / path).is_file() or digest((repo / path).read_bytes()) != record["sha256"]
        for path, record in files.items()
    )
    if changed_during_read:
        unknown.append({"kind": "checkout_changed_during_audit"})
    checks = {
        name: {"status": "fail" if rows else "pass", "findings": rows}
        for name, rows in violations.items()
    }
    if source_unknown and not source_findings:
        checks["source_paths"]["status"] = "unverified"
    checks["scope_and_drift"] = {"status": "unverified" if unknown else "pass", "findings": unknown}
    has_violations = any(violations.values())
    return {
        "schema_version": 1, "command": "audit", "observed_at": datetime.now(timezone.utc).isoformat(),
        "status": "fail" if has_violations else "unverified",
        "source": {"repo": str(repo), **before, "files": files},
        "policy": {"sha256": policy_hash, "observed_upstream_sha": policy["observed_upstream_sha"]},
        "checks": checks, "inventory": inventory,
        "scoped_removal": {"status": "fail" if has_violations else ("unverified" if unknown else "pass")},
        "terminal_baseline": {"status": "unverified", "reason": "Build and terminal runtime were not executed by this audit"},
        "terminal_only": {
            "status": "fail" if has_violations else "unverified",
            "reason": "Known retained subsystems" if has_violations else "Scoped static inspection cannot establish complete removal or native runtime correctness",
        },
        "scope_limits": policy["scope_limits"],
    }


def managed_candidate(repo: Path, destination: Path, target: str) -> Path:
    # Keep the selected worktree and index authoritative for the replay.
    if any(os.environ.get(name) for name in (
        "GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
    )):
        raise ValueError("Unset Git directory, worktree, index, or object-directory overrides before replay")

    worktree = destination.resolve()
    if worktree == repo or not worktree.is_dir():
        raise ValueError("--worktree must be a separate existing NVMD-managed task copy")

    listing = git(repo, "worktree", "list", "--porcelain", "-z").stdout.decode("utf-8", "surrogateescape")
    entries = [dict(field.partition(" ")[::2] for field in block.split("\0"))
               for block in listing.split("\0\0") if block]
    if not entries or "bare" in entries[0]:
        raise ValueError("Cannot identify the main checkout for this repository")
    main = Path(entries[0]["worktree"]).resolve()
    if worktree.parent != main / ".work" / "wt":
        raise ValueError("--worktree must be under the main checkout's .work/wt directory")

    entry = next((entry for entry in entries if Path(entry["worktree"]).resolve() == worktree), None)
    expected_branch = f"task/{worktree.name}"
    if entry is None or entry.get("branch") != f"refs/heads/{expected_branch}":
        raise ValueError("--worktree must be registered on its NVMD-managed task branch")
    if "locked" in entry or "prunable" in entry:
        raise ValueError("--worktree must be an active, unlocked NVMD-managed task copy")

    def common_git_dir(path: Path) -> Path:
        return Path(git(path, "rev-parse", "--path-format=absolute", "--git-common-dir")
                    .stdout.decode("utf-8", "surrogateescape").strip()).resolve()

    if common_git_dir(worktree) != common_git_dir(repo):
        raise ValueError("--worktree must share the source repository's Git directory")

    state = checkout_state(worktree)
    if entry.get("HEAD") != target or state["head"] != target or state["branch"] != expected_branch:
        raise ValueError("--worktree HEAD must be at the exact --target SHA on its task branch")
    if not state["clean"]:
        raise ValueError("--worktree must be clean before replay")
    return worktree


def candidate(args: argparse.Namespace, policy: dict, policy_hash: str) -> dict:
    repo = repo_root(args.repo)
    before = checkout_state(repo)
    result = {"schema_version": 1, "command": "candidate", "status": "fail",
              "preparation": {"status": "fail"}, "source_before": before}
    try:
        base = commit(repo, args.base)
        fork = commit(repo, args.fork)
        target = commit(repo, args.target)
        if before["head"] != fork:
            raise ValueError("Source HEAD differs from --fork; refresh the explicit SHA before retrying")
        if not before["clean"]:
            raise ValueError("Source checkout must be clean for candidate preparation")
        for descendant in (fork, target):
            if git(repo, "merge-base", "--is-ancestor", base, descendant, check=False).returncode:
                raise ValueError("--base must be an ancestor of both --fork and --target")
        patch = git(repo, "diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv", "--no-renames", base, fork, "--").stdout
        result["pins"] = {"base": base, "fork": fork, "target": target, "patch_sha256": digest(patch)}
        worktree = managed_candidate(repo, Path(args.worktree), target)
        result["worktree"] = str(worktree)
        if patch:
            applied = git(worktree, "apply", "--index", "--3way", "--binary", "-", input_data=patch, check=False)
            if applied.returncode:
                result["conflicts"] = git(worktree, "diff", "--name-only", "--diff-filter=U").stdout.decode().splitlines()
                result["patch_error"] = applied.stderr.decode("utf-8", errors="replace")[-6000:]
                raise ValueError("Patch replay failed; the isolated worktree was preserved for inspection")
        tree = git(worktree, "write-tree").stdout.decode().strip()
        result["pins"]["candidate_tree"] = tree
        if args.expected_tree and tree != args.expected_tree:
            raise ValueError("Candidate tree differs from --expected-tree")
        result["preparation"] = {"status": "pass", "patch_staged": bool(patch), "lfs_hydrated": False}
        result["audit"] = audit(worktree, policy, policy_hash)
        result["status"] = result["audit"]["status"]
    except (ValueError, RuntimeError, OSError, subprocess.TimeoutExpired) as error:
        result["error"] = str(error)
    finally:
        after = checkout_state(repo)
        result["source_after"] = after
        result["main_guard"] = {"status": "pass" if before == after else "fail"}
        if before != after:
            result["status"] = "fail"
            result["preparation"]["status"] = "fail"
            result["error"] = "Source checkout changed during candidate preparation"
    return result


def remote_observation(output: str, base: str, stable_pattern: str) -> dict:
    refs = {}
    symbolic_head = None
    for line in output.splitlines():
        value, separator, name = line.partition("\t")
        if not separator:
            raise ValueError("Unexpected ls-remote output")
        if value.startswith("ref: ") and name == "HEAD":
            symbolic_head = value[5:]
        elif SHA_PATTERN.fullmatch(value):
            refs[name] = value
        else:
            raise ValueError("Unexpected ls-remote reference value")
    if "HEAD" not in refs:
        raise ValueError("Upstream did not advertise a HEAD commit")
    tags = []
    for ref, object_sha in sorted(refs.items()):
        if not ref.startswith("refs/tags/") or ref.endswith("^{}"):
            continue
        name = ref.removeprefix("refs/tags/")
        tags.append({"name": name, "object_sha": object_sha,
                     "target_sha": refs.get(ref + "^{}", object_sha),
                     "stable_pattern_match": bool(re.fullmatch(stable_pattern, name))})
    return {
        "head": {"sha": refs["HEAD"], "symbolic_ref": symbolic_head},
        "branches": {ref: sha for ref, sha in refs.items() if ref.startswith("refs/heads/")},
        "tags": tags,
        "stable_tags": [tag for tag in tags if tag["stable_pattern_match"]],
        "comparison": {"base_sha": base, "head_equal_to_base": refs["HEAD"] == base,
                       "ancestry": "unverified", "reason": "ls-remote exposes ref identities, not commit ancestry"},
    }


def observe_upstream(args: argparse.Namespace, policy: dict) -> dict:
    repo = repo_root(args.repo)
    base = commit(repo, args.base)
    git(repo, "remote", "get-url", args.remote)
    observation = git(repo, "ls-remote", "--symref", "--", args.remote, "HEAD", "refs/heads/main", "refs/heads/master", "refs/tags/*", timeout=args.timeout)
    result = {
        "schema_version": 1, "command": "observe-upstream", "status": "pass",
        "observed_at": datetime.now(timezone.utc).isoformat(), "remote": args.remote,
        **remote_observation(observation.stdout.decode("utf-8"), base, policy["stable_tag_pattern"]),
    }
    if args.state:
        state = Path(args.state).resolve()
        if state.is_relative_to(repo) and git(repo, "ls-files", "--error-unmatch", "--", str(state.relative_to(repo)), check=False).returncode == 0:
            raise ValueError("--state must not overwrite a tracked source file")
        if state.exists():
            previous = json.loads(state.read_text(encoding="utf-8"))
            if previous.get("command") != "observe-upstream" or previous.get("status") != "pass":
                raise ValueError("--state already contains a different kind of file")
        state.parent.mkdir(parents=True, exist_ok=True)
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=state.parent, prefix=state.name + ".", suffix=".tmp", delete=False) as stream:
                temporary = Path(stream.name)
                json.dump(result, stream, ensure_ascii=False, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, state)
        finally:
            if temporary and temporary.exists():
                temporary.unlink()
        result["state_path"] = str(state)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--policy", type=Path, default=POLICY_PATH)
    commands = parser.add_subparsers(dest="command", required=True)
    audit_parser = commands.add_parser("audit", help="Read-only scoped source inventory and retained-subsystem checks")
    audit_parser.add_argument("--repo", default=".")
    candidate_parser = commands.add_parser("candidate", help="Replay a pinned fork delta in an existing NVMD-managed task copy")
    candidate_parser.add_argument("--repo", default=".")
    candidate_parser.add_argument("--base", required=True, help="Old upstream commit SHA")
    candidate_parser.add_argument("--fork", required=True, help="Expected current source HEAD SHA")
    candidate_parser.add_argument("--target", required=True, help="Already-local new upstream commit SHA")
    candidate_parser.add_argument("--worktree", required=True, help="Existing clean task copy at --target")
    candidate_parser.add_argument("--expected-tree", help="Optional tree SHA from an earlier receipt for exact reproduction")
    observer_parser = commands.add_parser("observe-upstream", help="Read remote refs without fetching or updating local refs")
    observer_parser.add_argument("--repo", default=".")
    observer_parser.add_argument("--base", required=True)
    observer_parser.add_argument("--remote", default="upstream")
    observer_parser.add_argument("--timeout", type=int, default=30, choices=range(1, 61), metavar="1..60")
    observer_parser.add_argument("--state", help="Atomically replace only the last successful observation JSON")
    args = parser.parse_args()
    try:
        raw_policy = args.policy.read_bytes()
        policy = json.loads(raw_policy)
        if policy["schema_version"] != 1:
            raise ValueError("Unsupported policy schema_version")
        if args.command == "audit":
            report = audit(repo_root(args.repo), policy, digest(raw_policy))
        elif args.command == "candidate":
            report = candidate(args, policy, digest(raw_policy))
        else:
            report = observe_upstream(args, policy)
    except (ValueError, RuntimeError, OSError, KeyError, re.error, subprocess.TimeoutExpired) as error:
        report = {"schema_version": 1, "command": args.command, "status": "unverified", "error": str(error)}
    print(json.dumps(report, ensure_ascii=True, indent=2))
    return {"pass": 0, "fail": 1, "unverified": 2}[report["status"]]


if __name__ == "__main__":
    sys.exit(main())
