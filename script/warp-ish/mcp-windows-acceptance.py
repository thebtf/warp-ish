"""Exercise the packaged MCP adapter against disposable Windows workspaces."""

from contextlib import ExitStack, contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
from queue import Queue
import shutil
import stat
import statistics
import subprocess
import sys
import threading
import time
from uuid import uuid4
import winreg


HERE = Path(__file__).resolve().parent
TIMEOUT = 40
CONFIG = "mcp-acceptance"
EXPECTED_TOOLS = {
    "where_am_i", "list_surfaces", "create_tab", "split_pane", "start_peer",
    "send_key", "close_pane", "close_tab",
}
PROFILE_ENV = "WARP_DATA_PROFILE"
PIPE_ENV = "WARP_ISH_MCP_PIPE"
UUID_ENV = "WARP_TERMINAL_SESSION_UUID"
BLOCK_ENV = "WARP_ISH_MCP_BLOCK_ID"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def await_result(read, label, timeout=TIMEOUT):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = read()
        if result:
            return result
        time.sleep(0.1)
    raise TimeoutError(f"Timed out waiting for {label}")


def safe_directory(path):
    for part in [path, *path.parents]:
        if part.exists():
            require(not part.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT,
                    f"Test path crosses a Windows reparse point: {part}")


def remove_fixture(path):
    if path.exists():
        safe_directory(path)
        require(path.resolve() == path, f"Unexpected fixture directory: {path}")
        shutil.rmtree(path)


def remove_settings_key(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
            children = [winreg.EnumKey(key, index) for index in range(winreg.QueryInfoKey(key)[0])]
    except FileNotFoundError:
        return
    for child in children:
        remove_settings_key(path + "\\" + child)
    winreg.DeleteKey(winreg.HKEY_CURRENT_USER, path)


def fingerprint(path):
    if not path.is_file():
        return None
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8-sig"))
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def ps_literal(text):
    return "'" + str(text).replace("'", "''") + "'"


class Profile:
    def __init__(self, scratch, home, roaming, local, seed, windows):
        self.name = f"acceptance-{uuid4().hex}"
        self.home = home / f".warp-ish-{self.name}"
        self.roaming = roaming / "warpish" / f"WarpIsh-{self.name}"
        self.local = local / "warpish" / f"WarpIsh-{self.name}"
        self.scratch = scratch / self.name
        self.markers = [self.scratch / f"pane-{index}.json" for index in range(windows)]
        for directory in (self.home, self.roaming, self.local, self.scratch):
            safe_directory(directory)
            require(not directory.exists(), f"Fixture already exists: {directory}")
        try:
            self.prepare(seed)
        except BaseException:
            self.close()
            raise

    def prepare(self, seed):
        self.scratch.mkdir()
        (self.local / "data").mkdir(parents=True)
        (self.local / "config").mkdir()
        (self.local / "config/settings.toml").write_text(
            "[general]\nrestore_session = true\n", encoding="utf-8")
        # This is Windows SecureStorage's DPAPI file for the fresh profile's
        # LocalControlMode. Its contents use the same serialized value as Settings > Scripting.
        setting = self.local / "data" / f"dev.warpish.WarpIsh-{self.name}-LocalControlMode"
        setting.write_bytes(seed.read_bytes())
        config_dir = self.roaming / "data/launch_configurations"
        config_dir.mkdir(parents=True)
        title = "MCP acceptance window"
        windows = []
        for marker in self.markers:
            command = (
                f"[ordered]@{{pipe=$env:{PIPE_ENV};uuid=$env:{UUID_ENV};profile=$env:{PROFILE_ENV}}}"
                f" | ConvertTo-Json -Compress | Set-Content -LiteralPath {ps_literal(marker)} -Encoding utf8"
            )
            windows.append({"tabs": [{"title": title, "layout": {
                "cwd": str(self.scratch), "commands": [{"exec": command}],
            }}]})
        (config_dir / f"{CONFIG}.yaml").write_text(
            json.dumps({"name": CONFIG, "windows": windows}), encoding="utf-8")

    def environment(self):
        env = os.environ.copy()
        for key in (PIPE_ENV, UUID_ENV, BLOCK_ENV, "WARP_ISH_IMPORT_LAUNCH"):
            env.pop(key, None)
        return dict(env, WARP_DATA_PROFILE=self.name, TMP=str(self.scratch), TEMP=str(self.scratch))

    def pane_environments(self):
        output = []
        for marker in self.markers:
            record = await_result(lambda: read_json(marker), f"PowerShell pane marker {marker.name}")
            require(record.get("profile") == self.name and record.get("uuid") and record.get("pipe"),
                    "Fixture pane did not inherit its profile, UUID, and enabled MCP endpoint")
            output.append(dict(self.environment(), **{PIPE_ENV: record["pipe"], UUID_ENV: record["uuid"]}))
        return output

    def close(self):
        for directory in (self.home, self.roaming, self.local):
            remove_fixture(directory)
        remove_settings_key(r"Software\Warp.dev\WarpIsh-" + self.name)


class ToolFailure(RuntimeError):
    pass


class Client:
    def __init__(self, adapter, env):
        self.sequence = 0
        self.replies = Queue()
        self.process = subprocess.Popen(
            [str(adapter)], env=env, cwd=adapter.parent, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, encoding="utf-8",
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        threading.Thread(target=self.read_output, daemon=True).start()
        try:
            hello = self.rpc("initialize", {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "warp-ish-acceptance", "version": "1"},
            })
            require(hello.get("protocolVersion") in {"2024-11-05", "2025-06-18"},
                    "Adapter returned an unsupported MCP protocol version")
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise

    def read_output(self):
        for line in self.process.stdout:
            received = time.perf_counter()
            try:
                self.replies.put((received, json.loads(line)))
            except json.JSONDecodeError as error:
                self.replies.put((received, error))
                break
        self.replies.put((time.perf_counter(), None))

    def send(self, value):
        self.process.stdin.write(json.dumps(value, ensure_ascii=False) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.sequence += 1
        self.send({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params})
        return self.sequence

    def reply(self, seq):
        self.last_reply_time, raw = self.replies.get(timeout=TIMEOUT)
        if isinstance(raw, Exception):
            raise raw
        require(raw is not None, "Adapter exited before sending its stdio response")
        require(raw.get("jsonrpc") == "2.0" and raw.get("id") == seq,
                "Adapter response did not match the request")
        require("error" not in raw, f"Adapter returned JSON-RPC error: {raw.get('error')}")
        return raw["result"]

    def rpc(self, method, params):
        return self.reply(self.request(method, params))

    def begin_tool(self, tool, **params):
        return self.request("tools/call", {"name": tool, "arguments": params})

    def finish_tool(self, seq):
        reply = self.reply(seq)
        text = reply["content"][0]["text"]
        if reply["isError"]:
            raise ToolFailure(text)
        return json.loads(text)

    def tool(self, name, **params):
        return self.finish_tool(self.begin_tool(name, **params))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=4)
        self.process.stdout.close()


@contextmanager
def started_client(adapter, env):
    client = Client(adapter, env)
    try:
        yield client
    finally:
        client.close()


def stale(call):
    try:
        call()
    except ToolFailure as error:
        require("stale_target" in str(error).lower(), f"Expected stale_target, received {error}")
    else:
        raise RuntimeError("Stale target was unexpectedly accepted")


def layout_ids(client):
    surfaces = client.tool("list_surfaces")
    return {
        "windows": sorted(entry["id"] for entry in surfaces["windows"]),
        "tabs": sorted((entry["id"], entry["window_id"]) for entry in surfaces["tabs"]),
        "panes": sorted((entry["id"], entry["tab_id"], entry["window_id"])
                        for entry in surfaces["panes"]),
    }


def start_peer(client, adapter, pane, output, prompt):
    output.mkdir()
    deadline = time.monotonic() + TIMEOUT
    while True:
        try:
            client.tool("start_peer", pane_id=pane, program=sys.executable,
                        args=["-I", "-B", "-u", str(HERE / "mcp-windows-peer.py"),
                              str(output), str(adapter)],
                        cwd=str(output), prompt=prompt)
            break
        except ToolFailure as error:
            # No retry after an unconfirmed transport result. This one specific
            # reply means GUI rejected submission while PowerShell bootstraps.
            if "target_state_conflict" not in str(error).lower() or time.monotonic() >= deadline:
                raise
            time.sleep(0.25)
    ready = await_result(lambda: read_json(output / "ready.json"), "peer CLI to start")
    require(ready["prompt"] == prompt and Path(ready["cwd"]) == output,
            "Peer CLI received the wrong prompt or working directory")
    require("location_error" not in ready, f"Peer MCP lookup failed: {ready.get('location_error')}")
    location = ready["location"]
    require(ready["inherited_block_id"] and location["block_id"] == ready["inherited_block_id"]
            and location["pane_id"] == pane,
            "Peer MCP lookup did not resolve its own pane and inherited active block")


def sample_idle(native, processes):
    before = [native.sample(pid) for pid in processes]
    start = time.monotonic()
    time.sleep(2)
    after = [native.sample(pid) for pid in processes]
    elapsed = time.monotonic() - start
    return {
        "private_bytes": sum(value["private_bytes"] for value in after),
        "handles": sum(value["handles"] for value in after),
        "idle_cpu_percent_of_one_core": (
            sum(post["cpu_seconds"] - pre["cpu_seconds"] for pre, post in zip(before, after))
            * 100 / elapsed
        ),
        "sample_seconds": elapsed,
    }


def measure(native, gui, adapter, env, count):
    with ExitStack() as resources:
        clients = [resources.enter_context(started_client(adapter, env)) for _ in range(count)]
        # Each client's first call establishes its lazy pipe connection.
        for client in clients:
            client.tool("where_am_i")
        result = sample_idle(native, [gui.pid, *(client.process.pid for client in clients)])
        samples = []
        for _ in range(5):
            pending = [(time.perf_counter(), client.begin_tool("list_surfaces"))
                       for client in clients]
            for client, (start, seq) in zip(clients, pending):
                client.finish_tool(seq)
                samples.append((client.last_reply_time - start) * 1_000)
        samples.sort()
        result.update({
            "clients": count, "latency_samples": len(samples),
            "latency_p50_ms": statistics.median(samples),
            "latency_p95_ms": samples[(len(samples) * 95 + 99) // 100 - 1],
            "latency_max_ms": max(samples),
        })
        return result


def run(native, desktop, gui, adapter, first, second, report):
    app = desktop.start(gui, [], first.environment(), first.scratch)
    await_result(lambda: desktop.windows(app), "normal GUI startup")
    report["normal_startup"] = True
    # An explicit profile launch URI opens windows without touching the global selector.
    desktop.start(gui, [f"warp-ish://launch/{CONFIG}"], first.environment(), first.scratch)
    other = desktop.start(gui, [f"warp-ish://launch/{CONFIG}"], second.environment(), second.scratch)
    a, a_sibling = first.pane_environments()
    b, = second.pane_environments()
    require(a[PIPE_ENV] != b[PIPE_ENV], "Distinct profiles exposed the same pipe")
    baseline = sample_idle(native, [app.pid])
    with (started_client(adapter, a) as primary, started_client(adapter, a_sibling) as sibling,
          started_client(adapter, b) as separate):
        names = {tool["name"] for tool in primary.rpc("tools/list", {})["tools"]}
        require(EXPECTED_TOOLS <= names, "Runtime MCP tool catalog is incomplete")
        here = primary.tool("where_am_i")
        require(here["block_id"] is None, "Uncorrelated PowerShell caller received a block ID")
        next_window = sibling.tool("where_am_i")
        other_here = separate.tool("where_am_i")
        require(here["instance_id"] == next_window["instance_id"]
                and here["window_id"] != next_window["window_id"]
                and here["instance_id"] != other_here["instance_id"]
                and here["terminal_uuid"] == a[UUID_ENV], "MCP returned the wrong pane instance")
        report["profiles"] = 2
        report["caller_windows"] = 2
        original = layout_ids(primary)
        other_layout = layout_ids(separate)
        with started_client(adapter, dict(a, **{UUID_ENV: b[UUID_ENV]})) as wrong:
            stale(lambda: wrong.tool("where_am_i"))
        stale(lambda: primary.tool("close_pane", pane_id=other_here["pane_id"]))
        stale(lambda: primary.tool("create_tab", window_id=other_here["window_id"]))
        require(layout_ids(primary) == original and layout_ids(separate) == other_layout,
                "Rejected cross-profile calls changed a layout")
        report["cross_profile_targets_rejected"] = True
        peer = primary.tool("split_pane", pane_id=here["pane_id"], direction="right")["pane"]["id"]
        require((peer, here["tab_id"], here["window_id"]) in layout_ids(primary)["panes"],
                "Split landed in an unexpected tab")
        prompt = "MCP fixture's peer prompt — Привет"
        peer_out = first.scratch / "peer cli"
        start_peer(primary, adapter, peer, peer_out, prompt)
        report["peer_block_correlated"] = True
        probe = primary.tool("create_tab", window_id=here["window_id"])
        tab, witness = probe["tab"]["id"], probe["pane"]["id"]
        require((tab, here["window_id"]) in layout_ids(primary)["tabs"],
                "New tab landed in an unexpected window")
        witness_out = first.scratch / "witness"
        start_peer(primary, adapter, witness, witness_out, "other tab")
        primary.tool("send_key", pane_id=peer, key="x")
        received = await_result(lambda: read_json(peer_out / "keys.json"), "one addressed terminal key")
        require(received == ["x"] and not (witness_out / "keys.json").exists(),
                "Addressed key reached the wrong pane or was duplicated")
        primary.tool("send_key", pane_id=peer, key="c", modifiers=["ctrl"])
        exit_marker = await_result(lambda: read_json(peer_out / "exit.json"), "addressed Ctrl+C")
        require(exit_marker == {"keys": ["x"], "interrupted": True}
                and not (witness_out / "exit.json").exists(), "Ctrl+C interrupted the wrong pane")
        primary.tool("send_key", pane_id=witness, key="c", modifiers=["ctrl"])
        await_result(lambda: read_json(witness_out / "exit.json"), "witness process to finish")
        primary.tool("close_pane", pane_id=peer)
        await_result(lambda: all(entry[0] != peer for entry in layout_ids(primary)["panes"]),
                     "addressed split to close")
        stale(lambda: primary.tool("close_pane", pane_id=peer))
        primary.tool("close_tab", tab_id=tab)
        await_result(lambda: layout_ids(primary) == original, "only the created tab to close")
        stale(lambda: primary.tool("close_tab", tab_id=tab))
        require(primary.tool("where_am_i") == here and layout_ids(separate) == other_layout,
                "A peer action changed the origin or a different profile")
        report["addressed_peer_lifecycle"] = True
        report["stale_ids_rejected"] = True
    report["measurements"] = {
        "gui_baseline": baseline, "one_client": measure(native, app, adapter, a, 1),
        "forty_clients": measure(native, app, adapter, a, 40),
    }
    marker_paths = [*first.markers, peer_out / "ready.json", witness_out / "ready.json"]
    markers = [(path.stat().st_mtime_ns, fingerprint(path)) for path in marker_paths]
    desktop.stop(app)
    restarted = desktop.start(gui, [], first.environment(), first.scratch)
    await_result(lambda: desktop.windows(restarted), "GUI relaunch without a launch URI")
    time.sleep(2)
    require(markers == [(path.stat().st_mtime_ns, fingerprint(path)) for path in marker_paths],
            "Ordinary relaunch executed a previous fixture command")
    report["no_replay_on_ordinary_relaunch"] = True
    desktop.stop(restarted)
    desktop.stop(other)


def main(args):
    package, scratch, evidence, seed, home, roaming, local = map(Path, args)
    gui, adapter = (package / "app" / name for name in ("warp-ish.exe", "warp-ish-mcp.exe"))
    report = {"result": "fail", "gui_sha256": fingerprint(gui),
              "adapter_sha256": fingerprint(adapter), "package": str(package)}
    result = evidence / "result.json"
    selector = local / "warpish/WarpIsh/config/active-import-profile"
    before = fingerprint(selector)
    try:
        spec = importlib.util.spec_from_file_location("native", HERE / "mcp-windows-native.py")
        native = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(native)
        with ExitStack() as resources:
            first = Profile(scratch, home, roaming, local, seed, 2)
            resources.callback(first.close)
            second = Profile(scratch, home, roaming, local, seed, 1)
            resources.callback(second.close)
            desktop = native.Desktop(scratch.name)
            resources.callback(desktop.close)
            run(native, desktop, gui, adapter, first, second, report)
        report["default_selector_unchanged"] = fingerprint(selector) == before
        require(report["default_selector_unchanged"], "Default warp-ish profile selector changed")
        report["result"] = "pass"
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        try:
            remove_fixture(scratch)
        except Exception as error:
            report["cleanup_error"] = str(error)
            report["result"] = "fail"
        result.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(result)
    return 0 if report["result"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
