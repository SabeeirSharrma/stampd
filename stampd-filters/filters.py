"""Transit server for stampd-filters.

Exposes user-defined filter hooks for cross-language calls from the engine.
Functions are registered with Transit's Python runtime.
"""

import json
import sys
import os

# Add transit Python runtime to path
transit_runtime_path = os.path.join(
    os.path.dirname(__file__),
    "..",
    "node_modules",
    "transit",
    "packages",
    "transit-py-runtime"
)
if os.path.exists(transit_runtime_path):
    sys.path.insert(0, transit_runtime_path)

from transit_server import TransitServer, register_function


def check_mail_from(args_json: str) -> str:
    """Filter hook for MAIL FROM stage.
    
    Called by engine when a new message is being submitted.
    Can accept, reject, or modify the sender.
    """
    args = json.loads(args_json)
    sender = args.get("sender", "")
    
    # Example: reject known spam senders
    blocked_senders = ["spam@example.com"]
    
    if sender in blocked_senders:
        return json.dumps({
            "action": "reject",
            "reason": "Sender blocked"
        })
    
    return json.dumps({
        "action": "accept",
        "sender": sender
    })


def check_rcpt_to(args_json: str) -> str:
    """Filter hook for RCPT TO stage.
    
    Called by engine when a recipient is being added.
    Can accept, reject, or modify the recipient.
    """
    args = json.loads(args_json)
    recipient = args.get("recipient", "")
    
    # Example: reject mail to specific addresses
    blocked_recipients = ["abuse@example.com"]
    
    if recipient in blocked_recipients:
        return json.dumps({
            "action": "reject",
            "reason": "Recipient blocked"
        })
    
    return json.dumps({
        "action": "accept",
        "recipient": recipient
    })


def check_data(args_json: str) -> str:
    """Filter hook for DATA stage.
    
    Called by engine when the message body is being processed.
    Can accept, reject, or modify the message.
    """
    args = json.loads(args_json)
    headers = args.get("headers", {})
    body = args.get("body", "")
    
    # Example: reject messages with suspicious content
    suspicious_patterns = ["viagra", "casino", "lottery"]
    
    for pattern in suspicious_patterns:
        if pattern in body.lower():
            return json.dumps({
                "action": "reject",
                "reason": f"Suspicious content detected: {pattern}"
            })
    
    return json.dumps({
        "action": "accept",
        "headers": headers,
        "body": body
    })


if __name__ == "__main__":
    server = TransitServer()
    
    # Register filter hooks
    register_function("checkMailFrom", check_mail_from)
    register_function("checkRcptTo", check_rcpt_to)
    register_function("checkData", check_data)
    
    server.start()
