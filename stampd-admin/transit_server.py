"""
transit-py-runtime — Python resident-process server for Transit.

Listens on a local TCP port (127.0.0.1 only) and dispatches
function calls from JS via a compact binary protocol.

Protocol (v0.1, little-endian):
  Header:  [version:1][type:1][request_id:4][payload_len:4]
  CALL_REQUEST payload:  [fn_name_len:2][fn_name:N][args_len:4][args_json:N]
  CALL_RESPONSE payload: [status:1][result_len:4][result_json:N]
  HEALTH_PING payload:   empty
  HEALTH_PONG payload:   empty

Zero external dependencies — uses only socket, struct, json, threading from stdlib.
"""

import json
import socket
import struct
import sys
import threading
from concurrent.futures import ThreadPoolExecutor

# ─── Protocol constants ───────────────────────────────────────────────────────

PROTOCOL_VERSION = 1
TYPE_CALL_REQUEST = 0x01
TYPE_CALL_RESPONSE = 0x02
TYPE_HEALTH_PING = 0x03
TYPE_HEALTH_PONG = 0x04

STATUS_OK = 0
STATUS_ERROR = 1

HEADER_SIZE = 10  # version(1) + type(1) + request_id(4) + payload_len(4)
HEADER_FORMAT = "<BBII"  # little-endian: version, type, request_id, payload_len


# ─── Function registry ────────────────────────────────────────────────────────

_functions = {}


def register_function(name, fn):
    """Register a function that can be called from JS.

    Args:
        name: Function name as it will be called from JS
        fn: Callable that takes a JSON string and returns a JSON string
    """
    _functions[name] = fn
    print(f"[transit-py] Registered function: {name}", file=sys.stderr)


# ─── Server ───────────────────────────────────────────────────────────────────

class TransitServer:
    """TCP server that dispatches function calls from JS."""

    def __init__(self):
        self._server_socket = None
        self._port = -1
        self._running = False
        self._executor = ThreadPoolExecutor(max_workers=32)
        self._lock = threading.Lock()

    @property
    def port(self):
        return self._port

    def start(self):
        """Start the server on an ephemeral port (127.0.0.1 only)."""
        # Bind to loopback only — never exposed to the network
        self._server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server_socket.bind(("127.0.0.1", 0))
        self._server_socket.listen(50)
        self._port = self._server_socket.getsockname()[1]
        self._running = True

        print(f"[transit-py] Server listening on 127.0.0.1:{self._port}", file=sys.stderr)
        print(f"[transit-py] Registered {len(_functions)} functions", file=sys.stderr)

        # Signal readiness: PORT=<port> so the parent process can read it
        print(f"PORT={self._port}")
        sys.stdout.flush()

        # Accept connections in a loop
        while self._running:
            try:
                client, addr = self._server_socket.accept()
                # Only allow loopback connections
                if addr[0] != "127.0.0.1":
                    client.close()
                    continue
                self._executor.submit(self._handle_client, client)
            except OSError:
                # Socket closed (e.g. during stop()) — exit the loop
                break

    def stop(self):
        """Stop the server gracefully."""
        self._running = False
        if self._server_socket:
            try:
                self._server_socket.close()
            except OSError:
                pass
        self._executor.shutdown(wait=False)
        print("[transit-py] Server stopped", file=sys.stderr)

    def _handle_client(self, client):
        """Handle a single client connection."""
        try:
            client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            with client:
                while self._running:
                    # Read header
                    header = self._recv_exact(client, HEADER_SIZE)
                    if header is None:
                        break

                    version, msg_type, request_id, payload_len = struct.unpack(
                        HEADER_FORMAT, header
                    )

                    if version != PROTOCOL_VERSION:
                        print(f"[transit-py] Bad version: {version}", file=sys.stderr)
                        break

                    # Read payload
                    payload = self._recv_exact(client, payload_len) if payload_len > 0 else b""

                    # Dispatch
                    if msg_type == TYPE_CALL_REQUEST:
                        self._handle_call(payload, request_id, client)
                    elif msg_type == TYPE_HEALTH_PING:
                        self._handle_health_ping(request_id, client)
                    else:
                        print(f"[transit-py] Unknown type: {msg_type}", file=sys.stderr)
        except Exception as e:
            if self._running:
                print(f"[transit-py] Client error: {e}", file=sys.stderr)

    def _handle_call(self, payload, request_id, client):
        """Handle a CALL_REQUEST message."""
        try:
            buf = memoryview(payload)

            # Read function name (uint16 LE length + bytes)
            fn_name_len = struct.unpack_from("<H", buf, 0)[0]
            fn_name = bytes(buf[2 : 2 + fn_name_len]).decode("utf-8")

            # Read args JSON (uint32 LE length + bytes)
            args_offset = 2 + fn_name_len
            args_len = struct.unpack_from("<I", buf, args_offset)[0]
            args_json = bytes(buf[args_offset + 4 : args_offset + 4 + args_len]).decode("utf-8")

            # Look up and call the function
            fn = _functions.get(fn_name)
            if fn is None:
                status = STATUS_ERROR
                result_json = json.dumps(
                    {"error": f"Function '{fn_name}' not found. Available: {list(_functions.keys())}"}
                )
            else:
                try:
                    result_json = fn(args_json)
                    status = STATUS_OK
                except Exception as e:
                    status = STATUS_ERROR
                    result_json = json.dumps({"error": str(e)})

            # Write response
            result_bytes = result_json.encode("utf-8")
            payload_size = 1 + 4 + len(result_bytes)  # status(1) + result_len(4) + result(N)
            resp = struct.pack(
                HEADER_FORMAT,
                PROTOCOL_VERSION,
                TYPE_CALL_RESPONSE,
                request_id,
                payload_size,
            )
            resp += struct.pack("<BI", status, len(result_bytes))
            resp += result_bytes

            client.sendall(resp)
        except Exception as e:
            print(f"[transit-py] Call handler error: {e}", file=sys.stderr)

    def _handle_health_ping(self, request_id, client):
        """Handle a HEALTH_PING message."""
        resp = struct.pack(HEADER_FORMAT, PROTOCOL_VERSION, TYPE_HEALTH_PONG, request_id, 0)
        client.sendall(resp)

    def _recv_exact(self, sock, n):
        """Receive exactly n bytes from a socket."""
        data = bytearray()
        while len(data) < n:
            chunk = sock.recv(n - len(data))
            if not chunk:
                return None
            data.extend(chunk)
        return bytes(data)


# ─── Convenience function ─────────────────────────────────────────────────────

def start_server():
    """Create and start a TransitServer. Blocks until stopped."""
    server = TransitServer()
    import signal

    def shutdown_handler(signum, frame):
        server.stop()
        sys.exit(0)

    signal.signal(signal.SIGTERM, shutdown_handler)
    signal.signal(signal.SIGINT, shutdown_handler)
    server.start()
