#!/usr/bin/env python3
"""Build probes, SQLite backups, and offline layout checks for warp-ish."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import sqlite3
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from contextlib import closing


BASELINE = "f7fc2f2a08a3aa446baa23960710e630c5defd6e"
RUNTIME_CHECKS = (
    "isolated_windows_launch",
    "interactive_powershell_and_conpty",
    "actual_shell_cwd_after_restart",
    "actual_shell_cwd_after_crash",
    "actual_shell_cwd_after_upgrade",
    "tui_input_resize_unicode_and_omp",
    "terminal_only_ui_and_settings",
    "process_attributed_network_trace",
)

# Deliberately select the terminal layout contract, not cloud/history contents.
COLUMNS = {
    "app": ("active_window_id",),
    "windows": ("id", "active_tab_index", "window_width", "window_height", "origin_x", "origin_y", "quake_mode", "fullscreen_state"),
    "tabs": ("id", "window_id", "custom_title", "color", "pinned"),
    "pane_nodes": ("id", "tab_id", "parent_pane_node_id", "flex", "is_leaf"),
    "pane_branches": ("pane_node_id", "horizontal"),
    "pane_leaves": ("pane_node_id", "kind", "is_focused", "custom_vertical_tabs_title"),
    "terminal_panes": ("id", "uuid", "cwd", "is_active"),
}
SQLITE_SIDECAR_SUFFIXES = ("-wal", "-shm", "-journal")
BACKUP_BUSY_SECONDS = 5
SQLITE_BOOLEANS = {"quake_mode", "pinned", "is_leaf", "horizontal", "is_focused", "is_active"}
SQLITE_INTEGERS = {"id", "active_window_id", "active_tab_index", "fullscreen_state", "window_id",
                   "tab_id", "pane_node_id", "parent_pane_node_id"}
NULLABLE_INTEGERS = {"active_window_id", "parent_pane_node_id"}


def check(name, status, details):
    return {"id": name, "status": status, "details": details}


def report(scope, checks):
    states = {item["status"] for item in checks}
    status = "FAIL" if "FAIL" in states else "NOT_RUN" if "NOT_RUN" in states else "PASS"
    return {
        "schema_version": 1,
        "scope": scope,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "status": status,
        "checks": checks,
        "runtime_acceptance": {
            "status": "NOT_RUN",
            "checks": [check(name, "NOT_RUN", "Requires the built Windows application and observed evidence.") for name in RUNTIME_CHECKS],
        },
    }


def run_probe(argv, cwd):
    """Never install Rust, perform a Git checkout, or start an application."""
    env = os.environ.copy()
    env.update({
        "RUSTUP_AUTO_INSTALL": "0",
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_LFS_SKIP_SMUDGE": "1",
    })
    try:
        result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", timeout=90, check=False)
        return {"command": argv, "exit_code": result.returncode,
                "stdout": result.stdout.strip(), "stderr": result.stderr.strip()}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": argv, "exit_code": None, "stdout": "", "stderr": str(error)}


def probe_check(name, argv, cwd):
    result = run_probe(argv, cwd)
    return check(name, "PASS" if result["exit_code"] == 0 else "FAIL", result)


def lfs_check(repo):
    result = run_probe(["git", "-c", "core.quotepath=false", "lfs", "ls-files", "--long"], repo)
    if result["exit_code"] != 0:
        return check("lfs_checkout", "FAIL", result)
    unready = []
    total = 0
    for line in result["stdout"].splitlines():
        match = re.fullmatch(r"([0-9a-f]{64}) ([*-]) (.+)", line)
        if not match:
            return check("lfs_checkout", "FAIL", {"reason": "Unrecognized git-lfs output", "line": line})
        total += 1
        relative_path = match.group(3)
        path = (repo / relative_path).resolve()
        if not path.is_relative_to(repo) or not path.is_file():
            unready.append(relative_path)
            continue
        with path.open("rb") as stream:
            is_pointer = stream.read(128).startswith(b"version https://git-lfs.github.com/spec/v1\n")
        if match.group(2) == "-" or is_pointer:
            unready.append(relative_path)
    return check("lfs_checkout", "FAIL" if unready else "PASS", {
        "tracked_files": total,
        "unhydrated_or_missing_count": len(unready),
        "unhydrated_or_missing_sample": unready[:20],
        "content_hashes_verified": False,
    })


def windows_native_checks(repo):
    if platform.system() != "Windows":
        return [check("windows_native_toolchain", "NOT_RUN", "Run preflight on Windows x64.")]
    program_files = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    vswhere = program_files / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = run_probe([str(vswhere), "-products", "*", "-latest", "-requires",
                              "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], repo)
    tools = []
    if installation["exit_code"] == 0 and installation["stdout"]:
        tools = list((Path(installation["stdout"]) / "VC/Tools/MSVC").glob("*/bin/Hostx64/x64/cl.exe"))
    compiler_ready = any(path.with_name("link.exe").is_file() for path in tools)
    checks = [check("msvc_x64", "PASS" if compiler_ready else "FAIL", {
        "installation_probe": installation, "compilers": [str(path) for path in tools],
    })]
    sdk_roots = {program_files / "Windows Kits/10"}
    if os.environ.get("WindowsSdkDir"):
        sdk_roots.add(Path(os.environ["WindowsSdkDir"]))
    versions = []
    for sdk_root in sdk_roots:
        for header in sdk_root.glob("Include/*/um/Windows.h"):
            version = header.parents[1].name
            if (sdk_root / "Lib" / version / "um/x64/kernel32.lib").is_file():
                versions.append(str(sdk_root / "Include" / version))
    checks.append(check("windows_sdk_x64", "PASS" if versions else "FAIL", {"header_and_library_versions": sorted(versions)}))
    return checks


def preflight(repo, baseline, min_free_gib):
    repo = repo.resolve(strict=True)
    checks = []
    root = run_probe(["git", "rev-parse", "--show-toplevel"], repo)
    if root["exit_code"] != 0 or Path(root["stdout"]).resolve() != repo:
        return report("build-preflight", [check("repository_root", "FAIL", root)])
    head = run_probe(["git", "rev-parse", "HEAD"], repo)
    dirty = run_probe(["git", "status", "--porcelain", "--untracked-files=normal"], repo)
    checks.append(check("repository_root", "PASS", {"root": str(repo), "head": head["stdout"],
                                                        "working_tree_entries": len(dirty["stdout"].splitlines())}))
    checks.append(probe_check("upstream_baseline_ancestor", ["git", "merge-base", "--is-ancestor", baseline, "HEAD"], repo))
    toolchain = tomllib.loads((repo / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    checks.append(probe_check("pinned_rustc", ["rustup", "run", toolchain, "rustc", "-vV"], repo))
    checks.append(probe_check("pinned_cargo", ["rustup", "run", toolchain, "cargo", "--version"], repo))
    for name, argv in (("git_lfs", ["git", "lfs", "version"]), ("cmake", ["cmake", "--version"]),
                       ("clang", ["clang", "--version"])):
        checks.append(probe_check(name, argv, repo))
    checks.append(lfs_check(repo))
    checks.extend(windows_native_checks(repo))
    free_gib = shutil.disk_usage(repo).free / 1024**3
    status = "FAIL" if min_free_gib is not None and free_gib < min_free_gib else "PASS"
    checks.append(check("disk_free", status, {"free_gib": round(free_gib, 2), "minimum_requested_gib": min_free_gib,
                                             "build_space_requirement_measured": False}))
    checks.append(check("python", "PASS", {"executable": sys.executable, "version": platform.python_version()}))
    result = report("build-preflight", checks)
    result["build_status"] = "NOT_RUN"
    result["platform"] = {"system": platform.system(), "machine": platform.machine()}
    return result


def require(condition, message):
    if not condition:
        raise ValueError(message)


def rows(conn, table):
    available = {row[1] for row in conn.execute(f'PRAGMA table_info("{table}")')}
    missing = set(COLUMNS[table]) - available
    require(not missing, f"Missing schema columns in {table}: {sorted(missing)}")
    names = ", ".join(f'"{name}"' for name in COLUMNS[table])
    records = [dict(row) for row in conn.execute(f'SELECT {names} FROM "{table}"')]
    for record in records:
        for name, value in record.items():
            if name in SQLITE_BOOLEANS:
                require(type(value) is int and value in (0, 1), f"Invalid SQLite boolean {table}.{name}")
            elif name in SQLITE_INTEGERS:
                require(value is None and name in NULLABLE_INTEGERS
                        or type(value) is int and -(2**31) <= value < 2**31,
                        f"Invalid SQLite integer {table}.{name}")
    return records


def keyed(items, field, name):
    result = {item[field]: item for item in items}
    require(len(result) == len(items), f"Duplicate keys in {name}")
    return result


def canonical_layout(conn):
    tables = {table: rows(conn, table) for table in COLUMNS}
    windows = keyed(tables["windows"], "id", "windows")
    tabs = keyed(tables["tabs"], "id", "tabs")
    nodes = keyed(tables["pane_nodes"], "id", "pane_nodes")
    leaves = keyed(tables["pane_leaves"], "pane_node_id", "pane_leaves")
    branches = keyed(tables["pane_branches"], "pane_node_id", "pane_branches")
    terminals = keyed(tables["terminal_panes"], "id", "terminal_panes")
    require(windows and tabs and terminals, "An empty terminal layout cannot establish persistence")
    require(len(tables["app"]) == 1, "Expected exactly one app state row")
    active_window_id = tables["app"][0]["active_window_id"]
    require(active_window_id is None or active_window_id in windows, "Active window is missing")
    require(all(tab["window_id"] in windows for tab in tabs.values()), "Orphan tab")
    require(all(node["tab_id"] in tabs for node in nodes.values()), "Orphan pane node")
    require(set(leaves) <= nodes.keys() and set(branches) <= nodes.keys(), "Orphan leaf or branch")
    require(set(terminals) == set(leaves), "The acceptance layout must contain terminal leaves only")
    seen = set()
    uuids = set()

    def read_node(node_id, tab_id):
        require(node_id not in seen, "Cycle or duplicated pane node")
        seen.add(node_id)
        node = nodes[node_id]
        require(node["tab_id"] == tab_id, "Pane parent crosses tab boundary")
        child_ids = sorted(item["id"] for item in nodes.values() if item["parent_pane_node_id"] == node_id)
        result = {"flex": node["flex"]}
        if node["is_leaf"]:
            require(node_id in leaves and node_id in terminals and node_id not in branches, "Leaf data missing or conflicting")
            require(not child_ids, "Leaf has children")
            leaf = leaves[node_id]
            terminal = terminals[node_id]
            require(leaf["kind"] == "terminal", "The acceptance layout contains a nonterminal pane")
            uuid = terminal["uuid"]
            require(isinstance(uuid, bytes) and len(uuid) == 16 and uuid not in uuids, "Invalid or duplicate terminal UUID")
            uuids.add(uuid)
            cwd = terminal["cwd"]
            require(isinstance(cwd, str) and bool(cwd.strip()), "Terminal CWD is NULL or empty")
            result.update({"kind": "terminal", "uuid": uuid.hex(), "cwd": cwd,
                           "is_active": bool(terminal["is_active"]), "is_focused": bool(leaf["is_focused"]),
                           "custom_vertical_tabs_title": leaf["custom_vertical_tabs_title"]})
        else:
            require(node_id in branches and node_id not in leaves, "Branch data missing or conflicting")
            require(len(child_ids) >= 2, "Split branch must have at least two children")
            result.update({"kind": "split", "horizontal": bool(branches[node_id]["horizontal"]),
                           "children": [read_node(child, tab_id) for child in child_ids]})
        return result

    result_windows = []
    window_ids = sorted(windows)
    for window_id in window_ids:
        window = windows[window_id]
        tab_ids = sorted(tab["id"] for tab in tabs.values() if tab["window_id"] == window_id)
        require(tab_ids, "Window has no tabs")
        require(0 <= window["active_tab_index"] < len(tab_ids), "Active tab index is out of range")
        result_tabs = []
        for tab_id in tab_ids:
            tab = tabs[tab_id]
            roots = [node["id"] for node in nodes.values() if node["tab_id"] == tab_id and node["parent_pane_node_id"] is None]
            require(len(roots) == 1, "Each tab must have exactly one pane root")
            result_tabs.append({"custom_title": tab["custom_title"], "color": tab["color"],
                                "pinned": bool(tab["pinned"]), "root": read_node(roots[0], tab_id)})
        normalized_window = {name: window[name] for name in COLUMNS["windows"] if name != "id"}
        normalized_window["quake_mode"] = bool(window["quake_mode"])
        normalized_window["tabs"] = result_tabs
        result_windows.append(normalized_window)
    require(seen == nodes.keys(), "Unreachable pane nodes, dangling parent, or cycle")
    result = {"active_window_index": window_ids.index(active_window_id) if active_window_id is not None else None,
              "windows": result_windows}
    validate_layout(result)
    return result


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def file_state(path):
    try:
        state = path.lstat()
    except FileNotFoundError:
        return None
    require(not stat.S_ISLNK(state.st_mode)
            and not getattr(state, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT,
            f"SQLite path is a symlink or reparse point: {path}")
    return state


def checked_backup_path(path):
    path = path.absolute()
    require(".." not in path.parts, f"SQLite path contains '..': {path}")
    if os.name == "nt":
        require(all(":" not in part and not part.endswith((".", " ")) for part in path.parts[1:]),
                f"SQLite path contains a Windows stream or ambiguous name: {path}")
    for parent in reversed(path.parents):
        state = file_state(parent)
        require(state is not None and stat.S_ISDIR(state.st_mode), f"SQLite parent is not a directory: {parent}")
    return path


def require_closed_copy(path):
    for suffix in SQLITE_SIDECAR_SUFFIXES:
        require(file_state(Path(str(path) + suffix)) is None,
                f"SQLite sidecar {suffix} exists. Supply a closed, standalone backup copy.")


def require_new_backup(path):
    require(file_state(path) is None, f"Snapshot already exists: {path}")
    require_closed_copy(path)


def require_sqlite_integrity(conn):
    require([row[0] for row in conn.execute("PRAGMA quick_check")] == ["ok"], "SQLite integrity check failed")


def backup_database(source, snapshot):
    source = checked_backup_path(source)
    snapshot = checked_backup_path(snapshot)
    state = file_state(source)
    require(state is not None and stat.S_ISREG(state.st_mode) and state.st_size > 0,
            f"Source must be a nonempty SQLite file: {source}")
    source = source.resolve(strict=True)
    snapshot = snapshot.parent.resolve(strict=True) / snapshot.name
    require_new_backup(snapshot)
    for suffix in SQLITE_SIDECAR_SUFFIXES:
        sidecar = Path(str(source) + suffix)
        require(sidecar != snapshot, f"Snapshot is a source SQLite sidecar: {snapshot}")
        state = file_state(sidecar)
        require(state is None or stat.S_ISREG(state.st_mode), f"Source SQLite sidecar must be a file: {sidecar}")

    with tempfile.TemporaryDirectory(prefix=".verify-sqlite-", dir=snapshot.parent) as temporary:
        pending = Path(temporary) / "backup.sqlite"
        with (
            closing(sqlite3.connect(source.as_uri() + "?mode=ro", uri=True, timeout=0.1)) as origin,
            closing(sqlite3.connect(pending, timeout=0.1)) as destination,
        ):
            since_progress = time.monotonic()
            # A live writer can restart an incremental backup. Only a new low-water
            # mark counts as progress, so repeated restarts cannot retry forever.
            fewest_remaining = None

            def check_progress(status, remaining, _total):
                nonlocal since_progress, fewest_remaining
                if status == sqlite3.SQLITE_DONE:
                    return
                now = time.monotonic()
                if status == sqlite3.SQLITE_OK and (fewest_remaining is None or remaining < fewest_remaining):
                    fewest_remaining = remaining
                    since_progress = now
                require(now - since_progress < BACKUP_BUSY_SECONDS,
                        f"Timed out waiting for SQLite backup progress: {source}")

            origin.backup(destination, pages=128, progress=check_progress, sleep=0.05)
            require(destination.execute("PRAGMA journal_mode=DELETE").fetchone() == ("delete",),
                    "Cannot make SQLite backup standalone")

        require_closed_copy(pending)
        with closing(sqlite3.connect(pending.as_uri() + "?mode=ro&immutable=1", uri=True)) as verified:
            require_sqlite_integrity(verified)
        require_new_backup(snapshot)
        # Both operations publish the verified file atomically and fail if the name exists.
        if os.name == "nt":
            pending.rename(snapshot)
        else:
            os.link(pending, snapshot)
    return snapshot


def capture_layout(database):
    path = database.resolve(strict=True)
    require(path.is_file(), "Database must be a file")
    require_closed_copy(path)
    digest_before = sha256(path)
    # immutable prevents SQLite from creating WAL/SHM files alongside the input.
    # The command accepts only a closed backup copy and rejects existing sidecars.
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True)) as conn:
        conn.row_factory = sqlite3.Row
        require_sqlite_integrity(conn)
        require(not list(conn.execute("PRAGMA foreign_key_check")), "SQLite foreign key check failed")
        layout = canonical_layout(conn)
    require_closed_copy(path)
    require(sha256(path) == digest_before, "Database changed during capture")
    return {"schema_version": 1, "kind": "warp-ish-layout-snapshot",
            "created_at": datetime.now(timezone.utc).isoformat(),
            "source": {"database": str(path), "sha256": digest_before}, "layout": layout}


def compare_layouts(before, after):
    for snapshot in (before, after):
        require(isinstance(snapshot, dict) and snapshot.get("schema_version") == 1
                and snapshot.get("kind") == "warp-ish-layout-snapshot", "Unsupported layout snapshot")
        source = snapshot.get("source")
        require(isinstance(source, dict) and isinstance(source.get("sha256"), str)
                and re.fullmatch(r"[0-9a-f]{64}", source["sha256"]), "Missing source database hash")
        validate_layout(snapshot.get("layout"))
    equal = before["layout"] == after["layout"]
    return report("persisted-layout-comparison", [check("persisted_layout_equal", "PASS" if equal else "FAIL", {
        "equal": equal,
        "before_database_sha256": before.get("source", {}).get("sha256"),
        "after_database_sha256": after.get("source", {}).get("sha256"),
        "actual_shell_cwd_verified": False,
    })])


def validate_layout(layout):
    """Reject malformed snapshots before equality can produce a passing check."""
    require(isinstance(layout, dict) and "active_window_index" in layout, "Missing layout object or active window field")
    windows = layout.get("windows")
    require(isinstance(windows, list) and windows, "Empty layout snapshot")
    active = layout.get("active_window_index")
    require(active is None or type(active) is int and 0 <= active < len(windows), "Invalid active window index")
    seen_uuids = set()

    def finite_number(value):
        return type(value) in (int, float) and -sys.float_info.max <= value <= sys.float_info.max

    def optional_text(value, field):
        require(value is None or isinstance(value, str), f"Invalid text field {field}")

    def validate_node(node):
        require(isinstance(node, dict) and "flex" in node, "Malformed pane node")
        require(node["flex"] is None or finite_number(node["flex"]) and node["flex"] >= 0, "Invalid pane flex")
        if node.get("kind") == "split":
            children = node.get("children")
            require(isinstance(node.get("horizontal"), bool), "Invalid split direction")
            require(isinstance(children, list) and len(children) >= 2, "Invalid split children")
            for child in children:
                validate_node(child)
            return
        require(node.get("kind") == "terminal", "Unsupported pane kind")
        uuid = node.get("uuid")
        require(isinstance(uuid, str) and re.fullmatch(r"[0-9a-f]{32}", uuid) and uuid not in seen_uuids,
                "Invalid or duplicate snapshot UUID")
        seen_uuids.add(uuid)
        require(isinstance(node.get("cwd"), str) and node["cwd"].strip() and "\x00" not in node["cwd"], "Missing or invalid snapshot CWD")
        require(isinstance(node.get("is_active"), bool) and isinstance(node.get("is_focused"), bool), "Invalid pane focus")
        require("custom_vertical_tabs_title" in node, "Missing pane title field")
        optional_text(node["custom_vertical_tabs_title"], "custom_vertical_tabs_title")

    for window in windows:
        require(isinstance(window, dict), "Invalid window")
        require(set(COLUMNS["windows"]) - {"id"} <= window.keys(), "Missing window fields")
        bounds = [window[name] for name in ("window_width", "window_height", "origin_x", "origin_y")]
        require(all(value is None for value in bounds) or all(finite_number(value) for value in bounds), "Invalid window bounds")
        if bounds[0] is not None:
            require(bounds[0] > 0 and bounds[1] > 0, "Window dimensions must be positive")
        require(isinstance(window["quake_mode"], bool), "Invalid quake mode")
        require(type(window["fullscreen_state"]) is int and window["fullscreen_state"] in (0, 1, 2), "Invalid fullscreen state")
        tabs = window.get("tabs")
        require(isinstance(tabs, list) and tabs, "Window has no tabs")
        require(type(window["active_tab_index"]) is int and 0 <= window["active_tab_index"] < len(tabs), "Invalid active tab")
        for tab in tabs:
            require(isinstance(tab, dict) and {"custom_title", "color", "pinned", "root"} <= tab.keys(), "Invalid tab")
            require(isinstance(tab["pinned"], bool), "Invalid pinned flag")
            optional_text(tab["custom_title"], "custom_title")
            optional_text(tab["color"], "color")
            validate_node(tab["root"])


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("preflight", help="Inspect source, installed build tools, LFS, and disk space")
    build.add_argument("--repo", type=Path, required=True)
    build.add_argument("--baseline", default=BASELINE)
    build.add_argument("--min-free-gib", type=float)
    backup = commands.add_parser("backup-database", help="Save a consistent standalone SQLite backup")
    backup.add_argument("--source", type=Path, required=True)
    backup.add_argument("--snapshot", type=Path, required=True)
    capture = commands.add_parser("capture-layout", help="Read a closed standalone SQLite backup copy")
    capture.add_argument("--database", type=Path, required=True)
    capture.add_argument("--snapshot", type=Path, required=True)
    compare = commands.add_parser("compare-layout", help="Compare persisted fields in two captured layouts")
    compare.add_argument("--before", type=Path, required=True)
    compare.add_argument("--after", type=Path, required=True)
    for command in (build, capture, compare):
        command.add_argument("--report", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.command == "backup-database":
        try:
            snapshot = backup_database(args.source, args.snapshot)
        except (OSError, ValueError, sqlite3.Error) as error:
            print(f"SQLite backup failed: {error}", file=sys.stderr)
            return 1
        print(f"SQLite backup created: {snapshot}")
        return 0
    inputs = [getattr(args, name) for name in ("database", "before", "after") if hasattr(args, name)]
    outputs = [args.report] + ([args.snapshot] if hasattr(args, "snapshot") else [])
    if len({path.resolve() for path in inputs + outputs}) != len(inputs + outputs):
        parser.error("Input and output paths must be distinct")
    for output in outputs:
        if output.exists() and any(path.exists() and os.path.samefile(output, path) for path in inputs):
            parser.error("Output aliases an input file")
    if args.command == "preflight" and args.min_free_gib is not None and args.min_free_gib < 0:
        parser.error("--min-free-gib must be nonnegative")
    try:
        if args.command == "preflight":
            result = preflight(args.repo, args.baseline, args.min_free_gib)
        elif args.command == "capture-layout":
            snapshot = capture_layout(args.database)
            write_json(args.snapshot, snapshot)
            result = report("offline-persistence-capture", [check("closed_sqlite_layout", "PASS", {
                "snapshot": str(args.snapshot.resolve()), "actual_shell_cwd_verified": False,
            })])
        else:
            before = json.loads(args.before.read_text(encoding="utf-8"))
            after = json.loads(args.after.read_text(encoding="utf-8"))
            result = compare_layouts(before, after)
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error, RecursionError) as error:
        result = report(args.command, [check("execution", "FAIL", {"error": str(error)})])
    try:
        write_json(args.report, result)
    except OSError as error:
        print(f"Unable to write report: {error}", file=sys.stderr)
        return 1
    print(f"{result['scope']}: {result['status']}; runtime acceptance: NOT_RUN", file=sys.stderr)
    return {"PASS": 0, "FAIL": 1, "NOT_RUN": 3}[result["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
