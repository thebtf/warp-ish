"""Isolated black-box pixel acceptance for warp-ish on Windows, including real OMP."""

import base64
from contextlib import ExitStack
import ctypes as ct
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import sys
import time
from uuid import uuid4

from PIL import Image, ImageChops


HERE = Path(__file__).resolve()
HARNESS = HERE.parent
COLORS = [(230, 46, 122), (46, 212, 120)]
SIZE = (120, 72)


def load(file):
    spec = importlib.util.spec_from_file_location(file.stem, file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def pattern():
    im = Image.new("RGB", SIZE, COLORS[0])
    half_w, half_h = SIZE[0] // 2, SIZE[1] // 2
    im.paste(COLORS[1], (half_w, 0, SIZE[0], half_h))
    im.paste(COLORS[1], (0, half_h, half_w, SIZE[1]))
    buf = io.BytesIO()
    im.save(buf, format="PNG")
    return buf.getvalue()


def sequences(kind):
    payload = base64.b64encode(pattern())
    if kind == "control":
        a, b = (b"\x1b[48;2;%d;%d;%dm" % color for color in COLORS)
        return (a + b"      " + b + b"      \x1b[0m\r\n") * 2 + (b + b"      " + a + b"      \x1b[0m\r\n") * 2
    if kind == "kitty":
        return b"\x1b_Ga=T,f=100,t=d,i=193,q=2;" + payload + b"\x1b\\"
    if kind == "kitty-crop":
        return (
            b"\x1b_Ga=t,f=100,i=193,q=2;"
            + payload
            + b"\x1b\\"
            + b"\x1b[4;5H\x1b_Ga=p,i=193,p=1,x=36,y=6,w=84,h=18,c=12,C=1,q=2\x1b\\"
            + b"\x1b[13;5H\x1b_Ga=p,i=193,p=2,x=0,y=36,w=80,h=36,c=12,C=1,q=2\x1b\\"
        )
    if kind == "kitty-virtual":
        # First indexes in the Kitty protocol's rowcolumn-diacritics table.
        marks = "\u0305\u030d\u030e\u0310\u0312\u033d\u033e\u033f\u0346\u034a"
        rows = [
            "\x1b[38;2;0;0;193m"
            + "".join("\U0010eeee" + row_mark + column_mark for column_mark in marks)
            + "\x1b[39m\r\n"
            for row_mark in marks[:4]
        ]
        return (
            b"\x1b_Ga=t,f=100,i=193,q=2;"
            + payload
            + b"\x1b\\"
            + b"\x1b_Ga=p,U=1,i=193,q=2,c=10,r=4\x1b\\"
            + "".join(rows).encode()
        )
    if kind == "kitty-multipart":
        chunks = [payload[start : start + 96] for start in range(0, len(payload), 96)]
        packets = []
        for idx, part in enumerate(chunks):
            prefix = b"a=T,f=100,t=d,i=193,q=2," if idx == 0 else b""
            more = b"m=0" if idx == len(chunks) - 1 else b"m=1"
            packets.append(b"\x1b_G" + prefix + more + b";" + part + b"\x1b\\")
        return b"".join(packets)
    if kind == "iterm":
        return b"\x1b]1337;File=inline=1;width=120px;height=72px:" + payload + b"\x07"
    if kind == "iterm-multipart":
        chunks = [payload[start : start + 96] for start in range(0, len(payload), 96)]
        return (
            b"\x1b]1337;MultipartFile=inline=1;width=120px;height=72px\x07"
            + b"".join(b"\x1b]1337;FilePart=" + part + b"\x07" for part in chunks)
            + b"\x1b]1337;FileEnd\x07"
        )
    if kind == "sixel":
        palettes = b"#0;2;90;18;48#1;2;18;83;47"
        # Each sixel column repeats the same color for all six pixels of its band.
        rows = [b"#0!60~#1!60~"] * 6 + [b"#1!60~#0!60~"] * 6
        return b'\x1bP0;0;0q"1;1;120;72' + palettes + b"-".join(rows) + b"\x1b\\"
    raise ValueError(kind)


def emit(kind, scratch, alt):
    from ctypes import wintypes as wt

    k = ct.WinDLL("kernel32", use_last_error=True)
    k.GetStdHandle.restype = wt.HANDLE
    out = wt.HANDLE(k.GetStdHandle(-11))
    mode = wt.DWORD()
    if not k.GetConsoleMode(out, ct.byref(mode)) or not k.SetConsoleMode(out, mode.value | 5):
        raise ct.WinError(ct.get_last_error())
    if kind == "kitty-virtual" and not k.SetConsoleOutputCP(65001):
        raise ct.WinError(ct.get_last_error())

    def write(data):
        done = wt.DWORD()
        if not k.WriteFile(out, data, len(data), ct.byref(done), None) or done.value != len(data):
            raise ct.WinError(ct.get_last_error())

    if alt:
        write(b"\x1b[?1049h\x1b[2J\x1b[4;4H")
    write(b"Image fixture\r\n")
    if kind == "sixel-clip":
        # Start three cells from the right edge. Only the left half of the
        # checker can fit here. Stretching to fit would expose the right half.
        column = os.get_terminal_size(sys.stdout.fileno()).columns - 2
        write(f"\x1b[{column}G".encode() + sequences("sixel"))
    else:
        write(sequences(kind))
    write(b"\r\n\r\n")
    (scratch / "drawn").touch()
    expires = time.monotonic() + 80
    while not (scratch / "advance").exists() and time.monotonic() < expires:
        time.sleep(0.1)
    if alt:
        write(b"\x1b[?1049l\r\nReturned to main screen\r\n")
    else:
        write(b"".join(f"Fixture scroll line {row:03}\r\n".encode() for row in range(90)))
    (scratch / "finished").touch()


def mask(image, color):
    diff = ImageChops.difference(image, Image.new("RGB", image.size, color))
    red, green, blue = diff.split()
    distance = ImageChops.lighter(ImageChops.lighter(red, green), blue)
    return distance.point([255 if value <= 12 else 0 for value in range(256)])


def find_pattern(image):
    bands = [mask(image, color) for color in COLORS]
    counts = [band.histogram()[255] for band in bands]
    if min(counts) < 36:
        return {"matches": False, "counts": counts}
    # Identify the shared diagonal boundary rather than just colors elsewhere
    # on screen. Require all four quadrant centers, in their correct order.
    pix = [band.load() for band in bands]
    bbox = bands[0].getbbox()
    for radius in (12, 7, 3):
        for y in range(max(radius, bbox[1]), min(image.height - radius, bbox[3]), radius):
            for x in range(max(radius, bbox[0]), min(image.width - radius, bbox[2]), radius):
                if (
                    pix[0][x - radius, y - radius]
                    and pix[1][x + radius, y - radius]
                    and pix[1][x - radius, y + radius]
                    and pix[0][x + radius, y + radius]
                ):
                    return {"matches": True, "center": [x, y], "counts": counts}
    return {"matches": False, "counts": counts}


def find_clipped_pattern(image):
    edge = image.crop((image.width * 9 // 10, 0, image.width, image.height))
    bands = [mask(edge, color) for color in COLORS]
    counts = [band.histogram()[255] for band in bands]
    pink, green = (band.getbbox() for band in bands)
    matches = bool(
        min(counts) >= 36
        and pink
        and green
        and abs(pink[0] - green[0]) <= 2
        and abs(pink[2] - green[2]) <= 2
        and abs(pink[3] - green[1]) <= 3
    )
    return {"matches": matches, "counts": counts, "boxes_in_right_tenth": [pink, green]}


def find_kitty_crops(image):
    pink, green = (mask(image, color) for color in COLORS)
    groups = []
    for y in range(image.height):
        row = (0, y, image.width, y + 1)
        a, b = pink.crop(row).getbbox(), green.crop(row).getbbox()
        if not a or not b or min(a[2] - a[0], b[2] - b[0]) < 6:
            continue
        if not groups or y - groups[-1][-1][0] > 2:
            groups.append([])
        pink_left = a[2] <= b[0] + 1
        green_left = b[2] <= a[0] + 1
        width = max(a[2], b[2]) - min(a[0], b[0])
        fraction = (a[2] - a[0]) / width
        groups[-1].append((y, pink_left, green_left, fraction))
    stripes = []
    for group in groups:
        if len(group) < 5:
            continue
        stripes.append(
            {
                "y": group[0][0],
                "height": group[-1][0] - group[0][0] + 1,
                "pink_left_rows": sum(row[1] for row in group) / len(group),
                "green_left_rows": sum(row[2] for row in group) / len(group),
                "pink_fraction": sum(row[3] for row in group) / len(group),
            }
        )
    # The upper source crop has 24 pink columns out of 84, all on the left.
    # The lower crop has 20 pink columns out of 80, all on the right. Placing
    # the complete image twice produces both orientations within each stripe.
    matches = False
    if len(stripes) == 2:
        top, bottom = stripes
        matches = (
            top["pink_left_rows"] >= 0.9
            and bottom["green_left_rows"] >= 0.9
            and abs(top["pink_fraction"] - 24 / 84) <= 0.08
            and abs(bottom["pink_fraction"] - 20 / 80) <= 0.08
            and bottom["height"] > top["height"] * 1.25
        )
    return {"matches": matches, "stripes": stripes}


def capture_window(hwnd):
    # Print exactly the HWND sent by the private desktop owner. Never read the
    # screen DC: a GPU surface on an inactive desktop can return unrelated pixels.
    from ctypes import wintypes as w

    user, gdi = ct.windll.user32, ct.windll.gdi32
    win = w.HWND(hwnd)
    rect = w.RECT()
    if not user.GetWindowRect(win, ct.byref(rect)):
        raise ct.WinError()
    width, height = rect.right - rect.left, rect.bottom - rect.top
    gdi.CreateCompatibleDC.restype = w.HDC
    gdi.CreateDIBSection.restype = w.HBITMAP
    gdi.SelectObject.restype = w.HGDIOBJ
    dc = w.HDC(gdi.CreateCompatibleDC(None))
    from struct import pack

    header = ct.create_string_buffer(pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0, 0, 0, 0, 0, 0))
    ptr = ct.c_void_p()
    bitmap = w.HBITMAP(gdi.CreateDIBSection(dc, ct.byref(header), 0, ct.byref(ptr), None, 0))
    if not bitmap:
        gdi.DeleteDC(dc)
        raise ct.WinError()
    prev = w.HGDIOBJ(gdi.SelectObject(dc, bitmap))
    try:
        if not user.PrintWindow(win, dc, 2):
            raise RuntimeError("PrintWindow failed for the owned test window")
        raw = ct.string_at(ptr, width * height * 4)
        return Image.frombytes("RGB", (width, height), raw, "raw", "BGRX")
    finally:
        gdi.SelectObject(dc, prev)
        gdi.DeleteObject(bitmap)
        gdi.DeleteDC(dc)


def inspect(hwnd, evidence, name, wanted, timeout=3, matcher=find_pattern):
    until = time.monotonic() + timeout
    while True:
        image = capture_window(hwnd)
        state = matcher(image)
        if state["matches"] == wanted or time.monotonic() >= until:
            image.save(evidence / (name + ".png"))
            return dict(state, size=image.size)
        time.sleep(0.25)


def session(scratch):
    pid = str(uuid4())
    when = "2026-11-25T00:00:00.000Z"
    encoded = base64.b64encode(pattern()).decode()
    rows = [dict(type="session", version=3, id=pid, timestamp=when, cwd=str(scratch))]
    entry = dict(
        role="assistant",
        api="openai-responses",
        provider="openai",
        model="gpt-5.2",
        stopReason="stop",
        content=[
            {"type": "text", "text": "Inline image acceptance"},
            {"type": "image", "mimeType": "image/png", "data": encoded},
            # OMP 19.2.1 omits a trailing assistant image on session replay.
            # Text below the PNG makes OMP write the actual image to its terminal.
            {"type": "text", "text": "End of fixture picture."},
        ],
        timestamp=1795564800000,
        usage=dict(
            input=0,
            output=0,
            cacheRead=0,
            cacheWrite=0,
            totalTokens=0,
            cost=dict(input=0, output=0, cacheRead=0, cacheWrite=0, total=0),
        ),
    )
    user_entry = dict(
        role="user", timestamp=1795564800000, content=[dict(type="text", text="Show the fixture image.")]
    )
    rows.extend(
        [
            dict(type="message", id="00000001", parentId=None, timestamp=when, message=user_entry),
            dict(type="message", id="00000002", parentId="00000001", timestamp=when, message=entry),
        ]
    )
    path = scratch / "fixture.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    return path


def window_resize(native, hwnd, height_only=False):
    w = native.wt
    rect = w.RECT()
    native.require(native.user.GetWindowRect(w.HWND(hwnd), ct.byref(rect)))
    w0, h0 = rect.right - rect.left, rect.bottom - rect.top
    width, height = (w0, h0 - 32) if height_only else (w0 - 65, h0)
    native.require(native.user.SetWindowPos(w.HWND(hwnd), None, 0, 0, width, height, 0x16))
    time.sleep(0.4)


def scroll(native, hwnd, steps):
    w = native.wt
    rect = w.RECT()
    native.require(native.user.GetWindowRect(w.HWND(hwnd), ct.byref(rect)))
    x, y = (rect.left + rect.right) // 2, (rect.top + rect.bottom) // 2
    pos = (x & 0xFFFF) | ((y & 0xFFFF) << 16)
    point = w.POINT(x, y)
    native.require(native.user.ScreenToClient(w.HWND(hwnd), ct.byref(point)))
    native.require(native.post_close(hwnd, 0x0200, 0, (point.x & 0xFFFF) | ((point.y & 0xFFFF) << 16)))
    time.sleep(0.25)
    for _ in range(abs(steps)):
        native.require(native.post_close(hwnd, 0x020A, (120 if steps > 0 else -120) * 65536, pos))
        time.sleep(0.04)


def run_case(fixture, native, package, omp, work, evidence, seed, folders, case):
    alt = case.endswith("-alt")
    kind = case.removesuffix("-alt")
    omp_case = kind == "omp" or kind.startswith("omp-")
    result = {"case": case}
    with ExitStack() as stack:
        profile = fixture.Profile(work, *folders, seed, 0)
        stack.callback(profile.close)
        desktop = native.Desktop(profile.name)
        stack.callback(desktop.close)
        scratch = profile.scratch
        env = profile.environment()
        for key in (
            "PI_FORCE_IMAGE_PROTOCOL",
            "PI_NO_KITTY_PLACEHOLDERS",
            "PI_KITTY_PLACEHOLDERS",
            "PI_PROFILE",
            "PI_CONFIG_FILES",
            "TERM_PROGRAM",
            "WT_SESSION",
            "WT_PROFILE_ID",
            "TERM",
            "OMP_PROFILE",
            "KITTY_WINDOW_ID",
            "VSCODE_PID",
            "WEZTERM_PANE",
            "ITERM_SESSION_ID",
            "GHOSTTY_RESOURCES_DIR",
        ):
            env.pop(key, None)
        quoted = fixture.ps_literal
        if omp_case:
            log = evidence / f"{kind}.tui.log"
            for name in ("omp-home", "omp-agent", "omp-natives"):
                (scratch / name).mkdir()
            env.update(
                OMP_PROFILE="",
                PI_CODING_AGENT_DIR=str(scratch / "omp-agent"),
                PI_CODING_AGENT_SESSION_DIR=str(scratch),
                PI_NATIVES_DIR=str(scratch / "omp-natives"),
                PI_CONFIG_DIR=".omp-inline-probe",
                USERPROFILE=str(scratch / "omp-home"),
                CLAUDE_CONFIG_DIR=str(scratch / "omp-home"),
                PI_TUI_WRITE_LOG=str(log),
                HTTP_PROXY="http://127.0.0.1:9",
                HTTPS_PROXY="http://127.0.0.1:9",
                ALL_PROXY="http://127.0.0.1:9",
                OTEL_SDK_DISABLED="true",
                PI_NOTIFICATIONS="off",
                PI_BUILD_URL="http://127.0.0.1:9",
                PI_GRAMMARS_URL="http://127.0.0.1:9",
            )
            if kind != "omp":
                env["PI_FORCE_IMAGE_PROTOCOL"] = kind.removeprefix("omp-")
            cmd = f"& {quoted(omp)} --session {quoted(session(scratch))} --no-extensions --no-tools --no-skills --no-context-files --no-title"
        else:
            cmd = f"& {quoted(sys.executable)} -I -B {quoted(HERE)} --emit {kind} {quoted(scratch)} " + (
                "alt" if alt else "main"
            )
        title = "Warp inline image fixture"
        config = {
            "name": fixture.CONFIG,
            "windows": [
                {"tabs": [{"title": title, "layout": {"cwd": str(scratch), "commands": [{"exec": cmd}]}}]}
            ],
        }
        (profile.roaming / f"data/launch_configurations/{fixture.CONFIG}.yaml").write_text(
            json.dumps(config), encoding="utf-8"
        )
        app = desktop.start(
            package / "app/warp-ish.exe", [f"warp-ish://launch/{fixture.CONFIG}"], env, scratch
        )
        hwnd = fixture.await_result(
            lambda: next((v["hwnd"] for v in desktop.describe_windows(app) if v["title"] == title), None),
            "owned fixture window",
            25,
        )
        old_desk = native.require(native.get_thread_desktop(native.get_thread_id()))
        native.require(native.set_thread_desktop(desktop.handle))
        stack.callback(native.set_thread_desktop, old_desk)
        if omp_case:
            fixture.await_result(lambda: log.is_file() and log.stat().st_size > 150, "real OMP writes", 45)
            time.sleep(3)
        else:
            fixture.await_result(lambda: (scratch / "drawn").exists(), "child marker", 35)
        matcher = {"sixel-clip": find_clipped_pattern, "kitty-crop": find_kitty_crops}.get(kind, find_pattern)
        result["first"] = inspect(hwnd, evidence, case + "-first", True, 5, matcher)
        if omp_case:
            data = log.read_bytes()
            result["omp"] = {
                "bytes": len(data),
                "kitty": data.count(b"\x1b_Ga="),
                "iterm": data.count(b"\x1b]1337;File="),
                "sixel": len(re.findall(rb"\x1bP[0-9;]*q", data)),
                "session_seen": b"Inline image acceptance" in data or b"fixture image" in data,
            }
        if result["first"]["matches"]:
            window_resize(native, hwnd, height_only=kind == "sixel-clip")
            result["resize"] = inspect(hwnd, evidence, case + "-resized", True, matcher=matcher)
        if not omp_case:
            (scratch / "advance").touch()
            fixture.await_result(lambda: (scratch / "finished").exists(), "writer exit", 8)
            time.sleep(0.6)
            result["offscreen"] = inspect(hwnd, evidence, case + "-offscreen", False, matcher=matcher)
            if not alt and result["first"]["matches"]:
                scroll(native, hwnd, 65)
                result["scroll_back"] = inspect(hwnd, evidence, case + "-restored", True)
        return result


def main(args):
    package, omp, work, evidence, seed, home, roaming, local = map(lambda p: Path(p).resolve(), args[:8])
    cases = args[8:]
    fixture = load(HARNESS / "mcp-windows-acceptance.py")
    native = load(HARNESS / "mcp-windows-native.py")
    awareness = native.bind(native.user, "SetThreadDpiAwarenessContext", ct.c_void_p, ct.c_void_p)
    initial = native.require(awareness(-4))
    results = {
        "application_sha256": fixture.fingerprint(package / "app/warp-ish.exe"),
        "omp_sha256": fixture.fingerprint(omp),
        "cases": [],
    }
    try:
        for case in cases:
            try:
                results["cases"].append(
                    run_case(
                        fixture, native, package, omp, work, evidence, seed, (home, roaming, local), case
                    )
                )
            except Exception as e:
                results["cases"].append({"case": case, "error": str(e)})
            (evidence / "result.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(results["cases"][-1]), flush=True)
    finally:
        awareness(initial)
        fixture.remove_fixture(work)

    def passed(item):
        output = item.get("omp")
        return (
            item.get("first", {}).get("matches")
            and item.get("resize", {}).get("matches")
            and ("offscreen" not in item or not item["offscreen"]["matches"])
            and ("scroll_back" not in item or item["scroll_back"]["matches"])
            and (
                not output or (output["session_seen"] and any(output[k] for k in ("sixel", "kitty", "iterm")))
            )
        )

    return int(not all(passed(item) for item in results["cases"]))


if __name__ == "__main__":
    if sys.argv[1] == "--emit":
        emit(sys.argv[2], Path(sys.argv[3]), sys.argv[4] == "alt")
    else:
        sys.exit(main(sys.argv[1:]))
