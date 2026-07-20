#!/usr/bin/env python3
"""
Block Sender Filter
===================
Reads JSON context from stdin, rejects if sender matches blocklist.

Config: Create a file named `blocklist.txt` in this filter directory,
one email address per line (case-insensitive).

Context JSON fields used:
  - hook: "mail_from"
  - sender: the envelope sender address
"""

import json
import sys
from pathlib import Path

def main():
    # Read context from stdin
    try:
        ctx = json.loads(sys.stdin.read())
    except json.JSONDecodeError:
        # Accept on parse error (fail open)
        print(json.dumps({"action": "accept"}))
        return

    sender = ctx.get("sender", "").lower()

    # Load blocklist
    blocklist_path = Path(__file__).parent / "blocklist.txt"
    if not blocklist_path.exists():
        print(json.dumps({"action": "accept"}))
        return

    blocked = set()
    for line in blocklist_path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            blocked.add(line.lower())

    if sender in blocked:
        print(json.dumps({"action": "reject", "reason": f"Sender {sender} is blocked"}))
    else:
        print(json.dumps({"action": "accept"}))

if __name__ == "__main__":
    main()
