"""Transit server for stampd-admin.

Exposes admin functions for cross-language calls from the gateway.
Functions are registered with Transit's Python runtime.
"""

import json
import sys
import os

# Add transit Python runtime to path
# In production, this would be installed as a dependency
transit_runtime_path = os.path.join(
    os.path.dirname(__file__),
    "..",
    "..",
    "node_modules",
    "transit",
    "packages",
    "transit-py-runtime"
)
if os.path.exists(transit_runtime_path):
    sys.path.insert(0, transit_runtime_path)

from transit_server import TransitServer, register_function


def get_users(args_json: str) -> str:
    """Get list of all users (admin only)."""
    # TODO: Query actual database
    result = {
        "users": [
            {"id": 1, "email": "admin@example.com", "is_admin": True},
        ],
        "total": 1
    }
    return json.dumps(result)


def create_user(args_json: str) -> str:
    """Create a new user account."""
    args = json.loads(args_json)
    email = args.get("email")
    password = args.get("password")
    
    # TODO: Hash password, insert into database
    result = {
        "success": True,
        "user": {"id": 1, "email": email}
    }
    return json.dumps(result)


def disable_user(args_json: str) -> str:
    """Disable a user account."""
    args = json.loads(args_json)
    user_id = args.get("user_id")
    
    # TODO: Update database
    result = {
        "success": True,
        "user_id": user_id,
        "disabled": True
    }
    return json.dumps(result)


def get_tokens(args_json: str) -> str:
    """Get auth tokens for a user."""
    args = json.loads(args_json)
    user_id = args.get("user_id")
    
    # TODO: Query database
    result = {
        "tokens": [],
        "total": 0
    }
    return json.dumps(result)


def create_token(args_json: str) -> str:
    """Create a new send-only auth token."""
    args = json.loads(args_json)
    user_id = args.get("user_id")
    label = args.get("label", "API token")
    
    # TODO: Generate token, hash, store in database
    result = {
        "success": True,
        "token": "raw-token-shown-once",
        "token_id": 1
    }
    return json.dumps(result)


def revoke_token(args_json: str) -> str:
    """Revoke an auth token."""
    args = json.loads(args_json)
    token_id = args.get("token_id")
    
    # TODO: Update database
    result = {
        "success": True,
        "token_id": token_id,
        "revoked": True
    }
    return json.dumps(result)


def get_server_config(args_json: str) -> str:
    """Get server configuration."""
    # TODO: Query database
    result = {
        "domain": "example.com",
        "signup_enabled": True,
        "dkim_selector": "default"
    }
    return json.dumps(result)


def update_server_config(args_json: str) -> str:
    """Update server configuration."""
    args = json.loads(args_json)
    
    # TODO: Update database, notify engine via Transit
    result = {
        "success": True,
        "message": "Config updated"
    }
    return json.dumps(result)


def get_delivery_logs(args_json: str) -> str:
    """Get recent delivery logs."""
    # TODO: Query database
    result = {
        "logs": [],
        "total": 0
    }
    return json.dumps(result)


if __name__ == "__main__":
    server = TransitServer()
    
    # Register all admin functions
    register_function("getUsers", get_users)
    register_function("createUser", create_user)
    register_function("disableUser", disable_user)
    register_function("getTokens", get_tokens)
    register_function("createToken", create_token)
    register_function("revokeToken", revoke_token)
    register_function("getServerConfig", get_server_config)
    register_function("updateServerConfig", update_server_config)
    register_function("getDeliveryLogs", get_delivery_logs)
    
    server.start()
