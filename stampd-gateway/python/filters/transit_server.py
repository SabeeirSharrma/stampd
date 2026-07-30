"""
transit-py-runtime — Python resident-process server for Transit.

Listens on a Unix domain socket (Linux/macOS) or local TCP port
(127.0.0.1 only, Windows) and dispatches function calls from JS
via a compact binary protocol.

Protocol (v0.1, little-endian):
  Header:  [version:1][type:1][request_id:4][payload_len:4]
  CALL_REQUEST payload:  [fn_name_len:2][fn_name:N][args_len:4][args_json:N]
  CALL_RESPONSE payload: [status:1][result_len:4][result_json:N]
  HEALTH_PING payload:   empty
  HEALTH_PONG payload:   empty

Transport auto-detection:
  - Linux/macOS: AF_UNIX at /tmp/transit-<pid>.sock (2-3x less latency)
  - Windows:     AF_INET on 127.0.0.1 (TCP loopback fallback)
  - Override:    Set TRANSIT_TRANSPORT=tcp to force TCP on any OS

Zero external dependencies — uses only socket, struct, json, threading from stdlib.
Optional: orjson for 2-10x faster JSON (set TRANSIT_USE_ORJSON=1).
"""

import atexit
import os
import socket
import struct
import sys
import threading
from concurrent.futures import ThreadPoolExecutor

# ─── orjson injection (opt-in via TRANSIT_USE_ORJSON=1) ────────────────────────
# Must happen BEFORE `import json` so user code that does `import json`
# transparently picks up orjson's faster implementation.

_using_orjson = False

if os.environ.get("TRANSIT_USE_ORJSON", "").strip() == "1":
    try:
        import orjson as _orjson
        import types

        # orjson.dumps() returns bytes, but stdlib json.dumps() returns str.
        # Create a compatibility shim so existing code doesn't break.
        _json_shim = types.ModuleType("json")
        _json_shim.loads = _orjson.loads  # type: ignore[attr-defined]

        def _dumps_compat(obj, **kwargs):
            """orjson.dumps wrapper that returns str (like stdlib json.dumps)."""
            return _orjson.dumps(obj).decode("utf-8")

        _json_shim.dumps = _dumps_compat  # type: ignore[attr-defined]

        # Inject into sys.modules so `import json` picks up the shim
        sys.modules["json"] = _json_shim
        _using_orjson = True
        print("[transit-py] orjson injected (2-10x faster JSON)", file=sys.stderr)
    except ImportError:
        print(
            "[transit-py] WARNING: TRANSIT_USE_ORJSON=1 but orjson is not installed. "
            "Falling back to stdlib json. Install with: pip install orjson",
            file=sys.stderr,
        )

import json

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
    """IPC server that dispatches function calls from JS.

    Uses Unix domain sockets on Linux/macOS for lower latency,
    falls back to TCP loopback on Windows.
    """

    def __init__(self):
        self._server_socket = None
        self._port = -1
        self._socket_path = None  # set when using UDS
        self._running = False
        self._executor = ThreadPoolExecutor(max_workers=min(os.cpu_count() or 4, 8))
        self._call_executor = ThreadPoolExecutor(max_workers=min(os.cpu_count() or 4, 8))
        self._lock = threading.Lock()

    @property
    def port(self):
        return self._port

    @property
    def socket_path(self):
        return self._socket_path

    def _use_uds(self):
        """Determine whether to use Unix domain sockets."""
        # Env override: TRANSIT_TRANSPORT=tcp forces TCP loopback
        if os.environ.get("TRANSIT_TRANSPORT", "").lower() == "tcp":
            return False
        # AF_UNIX not available on Windows (before 3.12+ with limited support)
        return hasattr(socket, "AF_UNIX") and os.name != "nt"

    def start(self):
        """Start the server.

        On Linux/macOS: binds to /tmp/transit-<pid>.sock (AF_UNIX).
        On Windows: binds to 127.0.0.1:<ephemeral> (AF_INET TCP loopback).
        """
        if self._use_uds():
            self._start_uds()
        else:
            self._start_tcp()

    def _start_uds(self):
        """Start the server on a Unix domain socket."""
        self._socket_path = f"/tmp/transit-{os.getpid()}.sock"

        # Clean up stale socket file from a previous crash
        if os.path.exists(self._socket_path):
            os.unlink(self._socket_path)

        self._server_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._server_socket.bind(self._socket_path)
        self._server_socket.listen(50)
        self._running = True

        # Register crash cleanup
        atexit.register(self._cleanup_socket_file)

        print(f"[transit-py] Server listening on UDS {self._socket_path}", file=sys.stderr)
        print(f"[transit-py] Registered {len(_functions)} functions", file=sys.stderr)

        # Signal readiness: SOCKET=<path> so the parent process can read it
        print(f"SOCKET={self._socket_path}")
        sys.stdout.flush()

        # Accept connections in a loop
        self._accept_loop()

    def _start_tcp(self):
        """Start the server on a TCP loopback port."""
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
        self._accept_loop()

    def _accept_loop(self):
        """Accept connections in a loop (shared by UDS and TCP)."""
        while self._running:
            try:
                client, addr = self._server_socket.accept()
                # For TCP, only allow loopback connections
                if self._socket_path is None and addr and addr[0] != "127.0.0.1":
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
        self._cleanup_socket_file()
        self._executor.shutdown(wait=False)
        self._call_executor.shutdown(wait=False)
        print("[transit-py] Server stopped", file=sys.stderr)

    def _cleanup_socket_file(self):
        """Remove the UDS socket file if it exists."""
        if self._socket_path and os.path.exists(self._socket_path):
            try:
                os.unlink(self._socket_path)
            except OSError:
                pass

    def _handle_client(self, client):
        """Handle a single client connection.

        Reads requests sequentially and dispatches CALL_REQUESTs to a
        separate call executor to avoid deadlock. The call executor is
        independent from the connection executor, so _handle_call tasks
        are not blocked by _handle_client tasks.
        """
        try:
            # TCP_NODELAY only applies to TCP sockets, not UDS
            if self._socket_path is None:
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

                    # Dispatch to separate call executor (avoids deadlock)
                    if msg_type == TYPE_CALL_REQUEST:
                        self._call_executor.submit(
                            self._handle_call, payload, request_id, client
                        )
                    elif msg_type == TYPE_HEALTH_PING:
                        self._handle_health_ping(request_id, client)
                    else:
                        print(f"[transit-py] Unknown type: {msg_type}", file=sys.stderr)
        except Exception as e:
            if self._running:
                print(f"[transit-py] Client error: {e}", file=sys.stderr)

    def _handle_call(self, payload, request_id, client):
        """Handle a CALL_REQUEST message.

        Processes the request and writes the response directly to the client socket.
        Called inline from _handle_client to avoid thread pool deadlock.
        """
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

            # Write response — single allocation via pack_into
            result_bytes = result_json.encode("utf-8")
            result_len = len(result_bytes)
            payload_size = 1 + 4 + result_len  # status(1) + result_len(4) + result(N)
            total_size = HEADER_SIZE + payload_size
            resp = bytearray(total_size)
            struct.pack_into(
                HEADER_FORMAT, resp, 0,
                PROTOCOL_VERSION,
                TYPE_CALL_RESPONSE,
                request_id,
                payload_size,
            )
            struct.pack_into("<BI", resp, HEADER_SIZE, status, result_len)
            resp[HEADER_SIZE + 5 :] = result_bytes

            client.sendall(resp)
        except Exception as e:
            print(f"[transit-py] Call handler error: {e}", file=sys.stderr)

    def _handle_health_ping(self, request_id, client):
        """Handle a HEALTH_PING message."""
        resp = struct.pack(HEADER_FORMAT, PROTOCOL_VERSION, TYPE_HEALTH_PONG, request_id, 0)
        client.sendall(resp)

    def _recv_exact(self, sock, n):
        """Receive exactly n bytes from a socket.

        Uses recv_into with a pre-allocated buffer and memoryview
        to avoid intermediate allocations (zero-copy receive).
        """
        buf = bytearray(n)
        view = memoryview(buf)
        offset = 0
        while offset < n:
            nbytes = sock.recv_into(view[offset:], n - offset)
            if nbytes == 0:
                return None
            offset += nbytes
        return bytes(buf)


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
