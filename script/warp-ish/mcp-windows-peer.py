"""Read keys in a fixture ConPTY and record the argument and input it received."""

import json
import msvcrt
import os
from pathlib import Path
import subprocess
import sys
import time


def write(path, value):
    pending = path.with_suffix(".pending")
    pending.write_text(json.dumps(value), encoding="utf-8")
    pending.replace(path)


def locate(adapter):
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "warp-ish-peer-acceptance", "version": "1"},
        }},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "where_am_i", "arguments": {}}},
    ]
    result = subprocess.run(
        [adapter], input="\n".join(json.dumps(frame) for frame in requests) + "\n",
        capture_output=True, text=True, encoding="utf-8", timeout=15, check=True,
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    reply = next(frame["result"] for frame in replies if frame.get("id") == 2)
    text = reply["content"][0]["text"]
    if reply["isError"]:
        raise RuntimeError(text)
    return json.loads(text)


out = Path(sys.argv[1])
ready = {
    "prompt": sys.argv[3], "cwd": str(Path.cwd()),
    "inherited_block_id": os.environ.get("WARP_ISH_MCP_BLOCK_ID"),
}
try:
    ready["location"] = locate(sys.argv[2])
except Exception as error:
    ready["location_error"] = str(error)
write(out / "ready.json", ready)
keys = []
interrupted = False
try:
    while True:
        if msvcrt.kbhit():
            key = msvcrt.getwch()
            if key == "\x03":
                interrupted = True
                break
            keys.append(key)
            write(out / "keys.json", keys)
        time.sleep(0.02)
except KeyboardInterrupt:
    interrupted = True
finally:
    write(out / "exit.json", {"keys": keys, "interrupted": interrupted})
