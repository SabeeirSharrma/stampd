#!/usr/bin/env python3
"""
Block Subject Keywords Filter
=============================
Rejects messages whose Subject header contains blocked keywords.

Config: Create a file named `keywords.txt` in this filter directory,
one keyword per line (case-insensitive substring match).

Context JSON fields used:
  - hook: "data"
  - headers: full message headers (including Subject)
"""

import json
import sys
from pathlib import Path


def main():
    try:
        ctx = json.loads(sys.stdin.read())
    except json.JSONDecodeError:
        print(json.dumps({"action": "accept"}))
        return

    headers = ctx.get("headers", "") or ""

    # Extract Subject line
    subject = ""
    for line in headers.split("\n"):
        if line.lower().startswith("subject:"):
            subject = line.split(":", 1)[1].strip()
            break

    if not subject:
        print(json.dumps({"action": "accept"}))
        return

    # Load keywords
    keywords_path = Path(__file__).parent / "keywords.txt"
    if not keywords_path.exists():
        print(json.dumps({"action": "accept"}))
        return

    keywords = []
    for line in keywords_path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            keywords.append(line.lower())

    subject_lower = subject.lower()
    for kw in keywords:
        if kw in subject_lower:
            print(json.dumps({
                "action": "reject",
                "reason": f"Subject contains blocked keyword: {kw}"
            }))
            return

    print(json.dumps({"action": "accept"}))


if __name__ == "__main__":
    main()
