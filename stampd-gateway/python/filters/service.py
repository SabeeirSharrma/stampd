"""
Stampd Filter Service — Transit Python bridge.

Registers filter functions that the gateway calls via Transit.
Each function receives a JSON string and returns a JSON string.
"""

import json
import sys
import os

# Add current directory to path so transit_server can be imported
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from transit_server import TransitServer, register_function


# ── Filter Functions ──────────────────────────────────────────

def block_sender(args_json: str) -> str:
    """Block emails from specific senders."""
    args = json.loads(args_json)
    sender = args.get("sender", "").lower()
    hook = args.get("hook", "")

    # Only check at MAIL FROM stage
    if hook != "mail_from":
        return json.dumps({"action": "accept", "reason": ""})

    # Load blocked senders from config
    config_path = os.path.join(os.path.dirname(__file__), "block_sender.json")
    blocked = []
    if os.path.exists(config_path):
        with open(config_path) as f:
            blocked = json.load(f).get("blocked", [])

    for pattern in blocked:
        if pattern.lower() in sender:
            return json.dumps({
                "action": "reject",
                "reason": f"Sender blocked: {pattern}"
            })

    return json.dumps({"action": "accept", "reason": ""})


def spam_keywords(args_json: str) -> str:
    """Reject emails with spammy keywords in subject."""
    args = json.loads(args_json)
    hook = args.get("hook", "")
    headers = args.get("headers", "")

    # Only check at DATA stage
    if hook != "data":
        return json.dumps({"action": "accept", "reason": ""})

    # Load spam keywords from config
    config_path = os.path.join(os.path.dirname(__file__), "spam_keywords.json")
    keywords = []
    if os.path.exists(config_path):
        with open(config_path) as f:
            keywords = json.load(f).get("keywords", [])

    # Extract subject from headers
    subject = ""
    for line in headers.split("\n"):
        if line.lower().startswith("subject:"):
            subject = line.split(":", 1)[1].strip()
            break

    subject_lower = subject.lower()
    for kw in keywords:
        if kw.lower() in subject_lower:
            return json.dumps({
                "action": "reject",
                "reason": f"Spam keyword detected: {kw}"
            })

    return json.dumps({"action": "accept", "reason": ""})


def rate_limit(args_json: str) -> str:
    """Basic rate limiting per sender IP."""
    args = json.loads(args_json)
    client_ip = args.get("client_ip", "")

    # Simple in-memory rate tracking (resets on restart)
    # In production, use Redis or similar
    if not hasattr(rate_limit, "_counts"):
        rate_limit._counts = {}

    import time
    now = time.time()
    window = 60  # 1 minute window
    max_per_window = 100  # max connections per window

    key = client_ip
    if key not in rate_limit._counts:
        rate_limit._counts[key] = []

    # Clean old entries
    rate_limit._counts[key] = [
        t for t in rate_limit._counts[key] if now - t < window
    ]

    if len(rate_limit._counts[key]) >= max_per_window:
        return json.dumps({
            "action": "reject",
            "reason": f"Rate limit exceeded for {client_ip}"
        })

    rate_limit._counts[key].append(now)
    return json.dumps({"action": "accept", "reason": ""})


def check_recipient(args_json: str) -> str:
    """Validate recipient addresses."""
    args = json.loads(args_json)
    recipient = args.get("recipient", "")
    hook = args.get("hook", "")

    # Only check at RCPT TO stage
    if hook != "rcpt_to":
        return json.dumps({"action": "accept", "reason": ""})

    # Reject if no @ sign
    if "@" not in recipient:
        return json.dumps({
            "action": "reject",
            "reason": "Invalid recipient address"
        })

    return json.dumps({"action": "accept", "reason": ""})


# ── Registration ──────────────────────────────────────────────

register_function("blockSender", block_sender)
register_function("spamKeywords", spam_keywords)
register_function("rateLimit", rate_limit)
register_function("checkRecipient", check_recipient)


if __name__ == "__main__":
    server = TransitServer()
    server.start()
