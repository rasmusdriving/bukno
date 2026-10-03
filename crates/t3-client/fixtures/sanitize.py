#!/usr/bin/env python3
"""Turn raw t3-probe recordings into fixtures that are safe to commit.

Keeps structure, IDs, enums, numbers and timestamps. Replaces every other
string (chat text, titles, paths, commands, tool output) with a same-length
placeholder capped at 80 characters, so no transcript or path reaches Git.

    python3 sanitize.py RAW.jsonl OUT.jsonl [STREAM]

With STREAM (such as subscribeThread), only that stream's records are kept.
"""
import json
import re
import sys

KEEP_KEYS = {
    "_tag", "kind", "type", "status", "visibility", "role", "location", "driver",
    "inputIntent", "requestKind", "model", "slug", "name", "displayName",
    "shortName", "instanceId", "providerInstanceId", "stream", "at", "version",
    "serverVersion", "environmentId", "label", "origin", "activityRunStatus",
    "settledOverride", "runtimeMode", "interactionMode", "policy", "os", "arch",
}
ID_KEY = re.compile(r"(^id$|Id$|Ids$|At$|Cursor$|cursor$)")
LOREM = ("Placeholder text replaces the recorded content so no transcript is kept. ") * 3


def placeholder(text):
    n = min(len(text), 80)
    return LOREM[:n] if n else ""


def scrub(value, key=None):
    if isinstance(value, dict):
        return {k: scrub(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [scrub(v, key) for v in value]
    if isinstance(value, str):
        if key in KEEP_KEYS or (key and ID_KEY.search(key)):
            return value
        return placeholder(value)
    return value


def main():
    raw, out = sys.argv[1], sys.argv[2]
    only = sys.argv[3] if len(sys.argv) > 3 else None
    with open(raw) as src, open(out, "w") as dst:
        for line in src:
            record = json.loads(line)
            # Keyring, theme and settings blocks are not needed by the client.
            value = record["value"]
            if only and record["stream"] != only:
                continue
            if record["stream"] == "server.getConfig":
                value = {"environment": value["environment"], "providers": value["providers"]}
            if value.get("kind") == "snapshot" and "projection" in value:
                # The client reads visibleTurnItems and runs. Execution nodes and
                # the second copy of the timeline are emptied to keep this small.
                value["projection"]["nodes"] = []
                value["projection"]["turnItems"] = []
            dst.write(json.dumps({"stream": record["stream"], "value": scrub(value)}) + "\n")


if __name__ == "__main__":
    main()
