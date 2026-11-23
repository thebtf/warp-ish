"""Harness tests using synthetic SQLite fixtures. No Warp process is started."""

from contextlib import closing
import copy
import importlib.util
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "script/warp-ish/verify.py"
SPEC = importlib.util.spec_from_file_location("warp_ish_verify", SCRIPT)
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)


def fixture(path, offset=0):
    """Reduced read schema from upstream f7fc2f2a, without app migrations."""
    with closing(sqlite3.connect(path)) as conn:
        conn.executescript("""
            CREATE TABLE app (id INTEGER PRIMARY KEY, active_window_id INTEGER);
            CREATE TABLE windows (id INTEGER PRIMARY KEY, active_tab_index INTEGER,
                window_width REAL, window_height REAL, origin_x REAL, origin_y REAL,
                quake_mode INTEGER, fullscreen_state INTEGER);
            CREATE TABLE tabs (id INTEGER PRIMARY KEY, window_id INTEGER, custom_title TEXT,
                color TEXT, pinned INTEGER);
            CREATE TABLE pane_nodes (id INTEGER PRIMARY KEY, tab_id INTEGER,
                parent_pane_node_id INTEGER, flex REAL, is_leaf INTEGER);
            CREATE TABLE pane_branches (id INTEGER PRIMARY KEY, pane_node_id INTEGER,
                horizontal INTEGER);
            CREATE TABLE pane_leaves (pane_node_id INTEGER, kind TEXT, is_focused INTEGER,
                custom_vertical_tabs_title TEXT, PRIMARY KEY(pane_node_id, kind));
            CREATE TABLE terminal_panes (id INTEGER PRIMARY KEY, uuid BLOB, cwd TEXT, is_active INTEGER);
        """)
        conn.execute("INSERT INTO app VALUES (1, ?)", (1 + offset,))
        conn.executemany("INSERT INTO windows VALUES (?, ?, 1100, 800, 20, 30, 0, 0)",
                         [(1 + offset, 1), (2 + offset, 0)])
        conn.executemany("INSERT INTO tabs VALUES (?, ?, ?, NULL, 0)", [
            (1 + offset, 1 + offset, "workspace"), (2 + offset, 1 + offset, "разделение"),
            (3 + offset, 2 + offset, "second window"),
        ])
        conn.executemany("INSERT INTO pane_nodes VALUES (?, ?, ?, ?, ?)", [
            (1 + offset, 1 + offset, None, None, 1),
            (2 + offset, 2 + offset, None, None, 0),
            (3 + offset, 2 + offset, 2 + offset, 0.5, 1),
            (4 + offset, 2 + offset, 2 + offset, 0.5, 1),
            (5 + offset, 3 + offset, None, None, 1),
        ])
        conn.execute("INSERT INTO pane_branches VALUES (?, ?, 1)", (1 + offset, 2 + offset))
        cwds = {1: r"D:\Dev\warp-ish", 3: r"D:\Dev\path with spaces", 4: "D:\\Dev\\каталог", 5: r"D:\Temp"}
        for node_id, cwd in cwds.items():
            conn.execute("INSERT INTO pane_leaves VALUES (?, 'terminal', ?, NULL)", (node_id + offset, int(node_id != 4)))
            conn.execute("INSERT INTO terminal_panes VALUES (?, ?, ?, 1)",
                         (node_id + offset, node_id.to_bytes(16, "big"), cwd))
        conn.commit()


class LayoutTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="warp-ish-fixture-", dir=ROOT)
        self.root = Path(self.directory.name)
        self.database = self.root / "input.sqlite"
        fixture(self.database)

    def tearDown(self):
        self.directory.cleanup()

    def modify(self, sql):
        with closing(sqlite3.connect(self.database)) as conn:
            conn.execute(sql)
            conn.commit()

    def test_sqlite_backup_fixture_roundtrip(self):
        before = verify.capture_layout(self.database)
        restored = self.root / "restored.sqlite"
        verify.backup_database(self.database, restored)
        after = verify.capture_layout(restored)
        result = verify.compare_layouts(before, after)
        self.assertEqual("PASS", result["status"])
        self.assertEqual("NOT_RUN", result["runtime_acceptance"]["status"])
        self.assertTrue(all(item["status"] == "NOT_RUN" for item in result["runtime_acceptance"]["checks"]))
        windows = before["layout"]["windows"]
        self.assertEqual(2, len(windows))
        self.assertEqual(3, sum(len(window["tabs"]) for window in windows))
        children = windows[0]["tabs"][1]["root"]["children"]
        self.assertEqual(r"D:\Dev\path with spaces", children[0]["cwd"])
        self.assertEqual("D:\\Dev\\каталог", children[1]["cwd"])

    def test_backup_cli_captures_committed_wal(self):
        expected_database = self.root / "expected.sqlite"
        fixture(expected_database)
        cwd = r"D:\Dev\new WAL directory"
        with closing(sqlite3.connect(expected_database)) as expected:
            expected.execute("UPDATE terminal_panes SET cwd = ? WHERE id = 1", (cwd,))
            expected.commit()

        snapshot = self.root / "WAL snapshot.sqlite"
        with closing(sqlite3.connect(self.database)) as writer:
            self.assertEqual(("wal",), writer.execute("PRAGMA journal_mode=WAL").fetchone())
            writer.execute("PRAGMA wal_autocheckpoint=0")
            original_file = verify.sha256(self.database)
            writer.execute("UPDATE terminal_panes SET cwd = ? WHERE id = 1", (cwd,))
            writer.commit()
            self.assertGreater(Path(str(self.database) + "-wal").stat().st_size, 0)
            self.assertEqual(original_file, verify.sha256(self.database))
            result = subprocess.run([sys.executable, str(SCRIPT), "backup-database",
                                     "--source", str(self.database), "--snapshot", str(snapshot)],
                                    capture_output=True, text=True, timeout=15)
            self.assertEqual(0, result.returncode, result.stderr)
            self.assertEqual(original_file, verify.sha256(self.database))

        actual = verify.capture_layout(snapshot)
        expected = verify.capture_layout(expected_database)
        self.assertEqual("PASS", verify.compare_layouts(expected, actual)["status"])
        with closing(sqlite3.connect(snapshot)) as reader:
            self.assertEqual(("delete",), reader.execute("PRAGMA journal_mode").fetchone())

    def test_backup_rejects_existing_output_and_sidecars(self):
        snapshot = self.root / "backup.sqlite"
        snapshot.write_bytes(b"existing output")
        with self.assertRaisesRegex(ValueError, "Snapshot already exists"):
            verify.backup_database(self.database, snapshot)
        self.assertEqual(b"existing output", snapshot.read_bytes())
        snapshot.unlink()
        sidecar = Path(str(snapshot) + "-wal")
        sidecar.write_bytes(b"existing sidecar")
        with self.assertRaisesRegex(ValueError, "SQLite sidecar -wal exists"):
            verify.backup_database(self.database, snapshot)
        self.assertFalse(snapshot.exists())
        self.assertEqual(b"existing sidecar", sidecar.read_bytes())

    def test_backup_rejects_linked_parent(self):
        linked = self.root / "link"
        try:
            linked.symlink_to(self.root, target_is_directory=True)
        except OSError as error:
            self.skipTest(f"Cannot create test symlink: {error}")
        try:
            with self.assertRaisesRegex(ValueError, "symlink or reparse point"):
                verify.backup_database(self.database, linked / "backup.sqlite")
            self.assertFalse((self.root / "backup.sqlite").exists())
        finally:
            linked.unlink()

    def test_backup_rejects_source_sidecar_output(self):
        snapshot = Path(str(self.database) + "-wal")
        with self.assertRaisesRegex(ValueError, "Snapshot is a source SQLite sidecar"):
            verify.backup_database(self.database, snapshot)
        self.assertFalse(snapshot.exists())

    def test_backup_cleans_up_on_corrupt_or_missing_source(self):
        bad = self.root / "corrupt.sqlite"
        bad.write_bytes(b"invalid SQLite data")
        snapshot = self.root / "backup.sqlite"
        with self.assertRaises(sqlite3.DatabaseError):
            verify.backup_database(bad, snapshot)
        with self.assertRaisesRegex(ValueError, "Source must be a nonempty SQLite file"):
            verify.backup_database(self.root / "missing.sqlite", snapshot)
        self.assertEqual(b"invalid SQLite data", bad.read_bytes())
        self.assertFalse(snapshot.exists())
        self.assertEqual([], list(self.root.glob(".verify-sqlite-*")))

    def test_backup_times_out_if_source_is_locked(self):
        snapshot = self.root / "backup.sqlite"
        with closing(sqlite3.connect(self.database)) as locker:
            locker.execute("BEGIN EXCLUSIVE")
            with patch.object(verify, "BACKUP_BUSY_SECONDS", 0):
                with self.assertRaisesRegex(ValueError, "Timed out waiting for SQLite backup progress"):
                    verify.backup_database(self.database, snapshot)
        self.assertFalse(snapshot.exists())
        self.assertEqual([], list(self.root.glob(".verify-sqlite-*")))

    def test_regenerated_numeric_ids_preserve_layout(self):
        second = self.root / "renumbered.sqlite"
        fixture(second, offset=100)
        result = verify.compare_layouts(verify.capture_layout(self.database), verify.capture_layout(second))
        self.assertEqual("PASS", result["status"])

    def test_cwd_change_fails_comparison(self):
        before = verify.capture_layout(self.database)
        self.modify("UPDATE terminal_panes SET cwd = 'C:\\Users\\test' WHERE id = 3")
        self.assertEqual("FAIL", verify.compare_layouts(before, verify.capture_layout(self.database))["status"])

    def test_null_cwd_rejected(self):
        self.modify("UPDATE terminal_panes SET cwd = NULL WHERE id = 3")
        with self.assertRaisesRegex(ValueError, "CWD"):
            verify.capture_layout(self.database)

    def test_empty_cwd_rejected(self):
        self.modify("UPDATE terminal_panes SET cwd = '' WHERE id = 3")
        with self.assertRaisesRegex(ValueError, "CWD"):
            verify.capture_layout(self.database)

    def test_duplicate_uuid_rejected(self):
        self.modify("UPDATE terminal_panes SET uuid = (SELECT uuid FROM terminal_panes WHERE id = 1) WHERE id = 3")
        with self.assertRaisesRegex(ValueError, "UUID"):
            verify.capture_layout(self.database)

    def test_dangling_parent_rejected(self):
        self.modify("UPDATE pane_nodes SET parent_pane_node_id = 999 WHERE id = 5")
        with self.assertRaises(ValueError):
            verify.capture_layout(self.database)

    def test_cross_tab_parent_rejected(self):
        self.modify("UPDATE pane_nodes SET parent_pane_node_id = 2 WHERE id = 5")
        with self.assertRaisesRegex(ValueError, "crosses tab"):
            verify.capture_layout(self.database)

    def test_split_direction_change_fails(self):
        before = verify.capture_layout(self.database)
        self.modify("UPDATE pane_branches SET horizontal = 0")
        self.assertEqual("FAIL", verify.compare_layouts(before, verify.capture_layout(self.database))["status"])

    def test_active_tab_change_fails(self):
        before = verify.capture_layout(self.database)
        self.modify("UPDATE windows SET active_tab_index = 0 WHERE id = 1")
        self.assertEqual("FAIL", verify.compare_layouts(before, verify.capture_layout(self.database))["status"])

    def test_out_of_range_active_tab_rejected(self):
        self.modify("UPDATE windows SET active_tab_index = 10 WHERE id = 1")
        with self.assertRaisesRegex(ValueError, "Active tab"):
            verify.capture_layout(self.database)

    def test_missing_terminal_row_rejected(self):
        self.modify("DELETE FROM terminal_panes WHERE id = 3")
        with self.assertRaises(ValueError):
            verify.capture_layout(self.database)

    def test_unknown_schema_rejected(self):
        self.modify("ALTER TABLE terminal_panes RENAME COLUMN cwd TO new_cwd")
        with self.assertRaisesRegex(ValueError, "Missing schema"):
            verify.capture_layout(self.database)

    def test_nonterminal_acceptance_pane_rejected(self):
        self.modify("UPDATE pane_leaves SET kind = 'ai_memory' WHERE pane_node_id = 3")
        with self.assertRaisesRegex(ValueError, "nonterminal"):
            verify.capture_layout(self.database)

    def test_no_database_or_sidecar_is_written(self):
        before = self.database.read_bytes()
        files = sorted(self.root.iterdir())
        verify.capture_layout(self.database)
        self.assertEqual(before, self.database.read_bytes())
        self.assertEqual(files, sorted(self.root.iterdir()))

    def test_sidecars_rejected(self):
        for suffix in ("-wal", "-shm", "-journal"):
            with self.subTest(suffix=suffix):
                sidecar = Path(str(self.database) + suffix)
                sidecar.touch()
                with self.assertRaisesRegex(ValueError, "sidecar"):
                    verify.capture_layout(self.database)
                sidecar.unlink()

    def test_missing_database_is_not_created(self):
        path = self.root / "missing.sqlite"
        with self.assertRaises(FileNotFoundError):
            verify.capture_layout(path)
        self.assertFalse(path.exists())

    def test_malformed_equal_snapshots_cannot_pass(self):
        value = {"schema_version": 1, "kind": "warp-ish-layout-snapshot", "layout": {"windows": [{}]}}
        with self.assertRaises(ValueError):
            verify.compare_layouts(value, value)

    def test_snapshot_missing_cwd_cannot_pass(self):
        value = copy.deepcopy(verify.capture_layout(self.database))
        del value["layout"]["windows"][0]["tabs"][0]["root"]["cwd"]
        with self.assertRaises(ValueError):
            verify.compare_layouts(value, value)

    def test_corrupt_sqlite_scalar_fields_rejected(self):
        changes = [
            "UPDATE windows SET quake_mode = 'garbage'",
            "UPDATE tabs SET pinned = 2",
            "UPDATE pane_nodes SET is_leaf = 2 WHERE id = 1",
            "UPDATE pane_branches SET horizontal = 'garbage'",
            "UPDATE terminal_panes SET is_active = 2",
            "UPDATE pane_leaves SET is_focused = -1",
            "UPDATE windows SET window_width = 'garbage'",
            "UPDATE windows SET window_width = 0",
            "UPDATE windows SET origin_x = 1e999",
            "UPDATE windows SET origin_x = NULL",
            "UPDATE pane_nodes SET flex = -0.5 WHERE id = 3",
            "UPDATE windows SET fullscreen_state = 99",
            "UPDATE windows SET active_tab_index = 'garbage'",
        ]
        for index, sql in enumerate(changes):
            with self.subTest(sql=sql):
                path = self.root / f"corrupt-{index}.sqlite"
                fixture(path)
                with closing(sqlite3.connect(path)) as conn:
                    conn.execute(sql)
                    conn.commit()
                with self.assertRaises(ValueError):
                    verify.capture_layout(path)

    def test_equal_corrupt_snapshot_numbers_and_flags_rejected(self):
        original = verify.capture_layout(self.database)
        bad_windows = [("window_width", "wide"), ("window_width", 0), ("window_height", -1), ("window_width", 10**400),
                       ("origin_x", float("inf")), ("origin_y", float("nan")), ("origin_x", None),
                       ("quake_mode", "false"), ("fullscreen_state", 99), ("active_tab_index", True)]
        for field, value in bad_windows:
            with self.subTest(field=field, value=value):
                snapshot = copy.deepcopy(original)
                snapshot["layout"]["windows"][0][field] = value
                with self.assertRaises(ValueError):
                    verify.compare_layouts(snapshot, snapshot)
        for field, value in [("flex", -1), ("flex", float("inf")), ("flex", "half"),
                             ("custom_vertical_tabs_title", 12), ("cwd", "D:\\bad\x00path")]:
            with self.subTest(field=field, value=value):
                snapshot = copy.deepcopy(original)
                snapshot["layout"]["windows"][0]["tabs"][0]["root"][field] = value
                with self.assertRaises(ValueError):
                    verify.compare_layouts(snapshot, snapshot)

    def test_equal_corrupt_snapshot_tab_fields_rejected(self):
        original = verify.capture_layout(self.database)
        for field, value in [("pinned", "false"), ("pinned", 1), ("custom_title", 7), ("color", [])]:
            with self.subTest(field=field):
                snapshot = copy.deepcopy(original)
                snapshot["layout"]["windows"][0]["tabs"][0][field] = value
                with self.assertRaises(ValueError):
                    verify.compare_layouts(snapshot, snapshot)

    def test_null_bounds_zero_flex_and_negative_origin_are_valid(self):
        self.modify("UPDATE windows SET window_width = NULL, window_height = NULL, origin_x = NULL, origin_y = NULL WHERE id = 1")
        self.modify("UPDATE windows SET origin_x = -2000 WHERE id = 2")
        self.modify("UPDATE pane_nodes SET flex = 0 WHERE id = 3")
        snapshot = verify.capture_layout(self.database)
        self.assertEqual("PASS", verify.compare_layouts(snapshot, snapshot)["status"])

    def test_nullable_child_flex_is_valid(self):
        self.modify("UPDATE pane_nodes SET flex = NULL WHERE id = 3")
        snapshot = verify.capture_layout(self.database)
        self.assertEqual("PASS", verify.compare_layouts(snapshot, snapshot)["status"])

    def test_window_and_tab_order_changes_fail(self):
        original = verify.capture_layout(self.database)
        reordered_windows = copy.deepcopy(original)
        reordered_windows["layout"]["windows"].reverse()
        self.assertEqual("FAIL", verify.compare_layouts(original, reordered_windows)["status"])
        reordered_tabs = copy.deepcopy(original)
        reordered_tabs["layout"]["windows"][0]["tabs"].reverse()
        self.assertEqual("FAIL", verify.compare_layouts(original, reordered_tabs)["status"])

    def test_source_row_order_does_not_override_id_order(self):
        original_rows = verify.rows

        def reversed_rows(conn, table):
            return list(reversed(original_rows(conn, table)))

        original = verify.capture_layout(self.database)
        with patch.object(verify, "rows", side_effect=reversed_rows):
            reordered_query = verify.capture_layout(self.database)
        self.assertEqual("PASS", verify.compare_layouts(original, reordered_query)["status"])

    def test_cli_report_and_nonzero_failure(self):
        self.modify("UPDATE terminal_panes SET cwd = NULL WHERE id = 3")
        report_path = self.root / "report.json"
        result = subprocess.run([sys.executable, str(SCRIPT), "capture-layout", "--database", str(self.database),
                                 "--snapshot", str(self.root / "snapshot.json"), "--report", str(report_path)],
                                capture_output=True, text=True, check=False)
        self.assertEqual(1, result.returncode)
        self.assertEqual("FAIL", json.loads(report_path.read_text())["status"])
        self.assertFalse((self.root / "snapshot.json").exists())

    def test_output_cannot_overwrite_database(self):
        before = self.database.read_bytes()
        result = subprocess.run([sys.executable, str(SCRIPT), "capture-layout", "--database", str(self.database),
                                 "--snapshot", str(self.database), "--report", str(self.root / "report.json")],
                                capture_output=True, text=True, check=False)
        self.assertEqual(2, result.returncode)
        self.assertEqual(before, self.database.read_bytes())

    def test_output_hardlink_cannot_overwrite_database(self):
        alias = self.root / "alias.sqlite"
        alias.hardlink_to(self.database)
        before = self.database.read_bytes()
        result = subprocess.run([sys.executable, str(SCRIPT), "capture-layout", "--database", str(self.database),
                                 "--snapshot", str(alias), "--report", str(self.root / "report.json")],
                                capture_output=True, text=True, check=False)
        self.assertEqual(2, result.returncode)
        self.assertEqual(before, self.database.read_bytes())


class ProbeTests(unittest.TestCase):
    def test_rustup_probes_disable_automatic_installs(self):
        completed = subprocess.CompletedProcess([], 1, "", "toolchain not installed")
        with patch.object(verify.subprocess, "run", return_value=completed) as run:
            result = verify.run_probe(["rustup", "run", "1.92.0", "rustc", "-vV"], ROOT)
        self.assertEqual("0", run.call_args.kwargs["env"]["RUSTUP_AUTO_INSTALL"])
        self.assertEqual("0", run.call_args.kwargs["env"]["GIT_OPTIONAL_LOCKS"])
        self.assertEqual(1, result["exit_code"])

    def test_missing_command_reports_failure(self):
        with patch.object(verify.subprocess, "run", side_effect=FileNotFoundError("not installed")):
            self.assertEqual("FAIL", verify.probe_check("tool", ["missing-tool"], ROOT)["status"])

    def test_incomplete_report_is_not_pass(self):
        value = verify.report("build-preflight", [verify.check("windows", "NOT_RUN", "Linux runner")])
        self.assertEqual("NOT_RUN", value["status"])

    def test_lfs_pointer_reports_failure(self):
        with tempfile.TemporaryDirectory(prefix="warp-ish-lfs-fixture-", dir=ROOT) as directory:
            repo = Path(directory)
            (repo / "asset.bin").write_text("version https://git-lfs.github.com/spec/v1\n")
            response = {"exit_code": 0, "stdout": "a" * 64 + " - asset.bin", "stderr": ""}
            with patch.object(verify, "run_probe", return_value=response):
                result = verify.lfs_check(repo)
            self.assertEqual("FAIL", result["status"])
            self.assertEqual(1, result["details"]["unhydrated_or_missing_count"])


if __name__ == "__main__":
    unittest.main()
