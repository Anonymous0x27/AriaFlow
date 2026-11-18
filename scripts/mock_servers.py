#!/usr/bin/env python3
"""Local protocol mock servers for AriaFlow end-to-end tests.

Serves PAYLOAD_DIR over:
  - HTTP      : python's http.server on $HTTP_PORT      (payloads + files.metalink)
  - HTTP throttled: rate-limited download on $THROTTLE_PORT (pause/resume test)
  - FTP       : pyftpdlib anonymous on $FTP_PORT
  - SFTP      : paramiko password auth (any password, user 'ariaflow') on $SFTP_PORT

Payload layout (created if missing):
  $PAYLOAD_DIR/payload.bin   — 2 MiB deterministic bytes (seeded)
  $PAYLOAD_DIR/files.metalink — sha256-checked mirror list pointing at the HTTP server
"""
import hashlib
import os
import posixpath
import random
import socket
import stat
import sys
import threading

PAYLOAD_DIR = os.environ.get("PAYLOAD_DIR", "/tmp/ariaflow-e2e/payload")
HTTP_PORT = int(os.environ.get("HTTP_PORT", "18901"))
THROTTLE_PORT = int(os.environ.get("THROTTLE_PORT", "18904"))
FTP_PORT = int(os.environ.get("FTP_PORT", "18902"))
SFTP_PORT = int(os.environ.get("SFTP_PORT", "18903"))
os.makedirs(PAYLOAD_DIR, exist_ok=True)

PAYLOAD = os.path.join(PAYLOAD_DIR, "payload.bin")
if not os.path.exists(PAYLOAD):
    random.seed(42)
    with open(PAYLOAD, "wb") as f:
        f.write(random.randbytes(2 * 1024 * 1024))

DIGEST = hashlib.sha256(open(PAYLOAD, "rb").read()).hexdigest()

with open(os.path.join(PAYLOAD_DIR, "files.metalink"), "w") as f:
    f.write(f"""<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="payload.bin">
    <size>{os.path.getsize(PAYLOAD)}</size>
    <hash type="sha256">{DIGEST}</hash>
    <url preference="1">http://127.0.0.1:{HTTP_PORT}/payload.bin</url>
  </file>
</metalink>
""")

# ---------- HTTP (payload + metalink document) ----------
import http.server
import functools
import time

def _range_bounds(header, size):
    """Parse a single-range `bytes=start-end` / `bytes=start-` header.

    Returns (start, end_inclusive) or (0, size - 1) when absent/unsatisfiable.
    """
    if not header or not header.startswith("bytes="):
        return 0, size - 1
    spec = header[len("bytes="):].split(",")[0].strip()
    start_s, _, end_s = spec.partition("-")
    try:
        start = int(start_s) if start_s else max(0, size - int(end_s))
        end = int(end_s) if end_s else size - 1
    except ValueError:
        return 0, size - 1
    return max(0, min(start, size - 1)), min(end, size - 1)

class RangedFile(http.server.BaseHTTPRequestHandler):
    """Full-speed static file with proper Range support and HEAD."""

    CHUNK = 65536

    def _open(self):
        path = posixpath.normpath(self.path.split("?")[0])
        return os.path.join(PAYLOAD_DIR, path.lstrip("/"))

    def _send_range(self, target, include_body):
        if not os.path.isfile(target):
            self.send_error(404)
            return
        size = os.path.getsize(target)
        range_hdr = self.headers.get("Range")
        start, end = _range_bounds(range_hdr, size)
        status = 206 if range_hdr else 200
        length = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        if not include_body:
            return
        with open(target, "rb") as f:
            f.seek(start)
            remaining = length
            while remaining > 0:
                chunk = f.read(min(self.CHUNK, remaining))
                if not chunk:
                    break
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return
                remaining -= len(chunk)
                self.maybe_throttle()

    def maybe_throttle(self):
        pass

    def do_GET(self):
        self._send_range(self._open(), include_body=True)

    def do_HEAD(self):
        self._send_range(self._open(), include_body=False)

    def log_message(self, *args):
        pass

class Throttled(RangedFile):
    def maybe_throttle(self):
        time.sleep(0.25)  # ~256 KiB/s in 64 KiB chunks

http_thread = threading.Thread(
    target=lambda: http.server.ThreadingHTTPServer(
        ("127.0.0.1", HTTP_PORT), RangedFile
    ).serve_forever(),
    daemon=True,
)
http_thread.start()

throttle_thread = threading.Thread(
    target=lambda: http.server.ThreadingHTTPServer(("127.0.0.1", THROTTLE_PORT), Throttled).serve_forever(),
    daemon=True,
)
throttle_thread.start()

# ---------- FTP (anonymous) ----------
from pyftpdlib.authorizers import DummyAuthorizer
from pyftpdlib.handlers import FTPHandler
from pyftpdlib.servers import FTPServer

authorizer = DummyAuthorizer()
authorizer.add_anonymous(PAYLOAD_DIR)
ftp_handler = FTPHandler
ftp_handler.authorizer = authorizer
ftp_thread = threading.Thread(
    target=lambda: FTPServer(("127.0.0.1", FTP_PORT), FTPHandler).serve_forever(),
    daemon=True,
)
ftp_thread.start()

# ---------- SFTP (password auth, any password) ----------
import paramiko

HOST_KEY = paramiko.RSAKey.generate(2048)

class SFTPServerInterface(paramiko.SFTPServerInterface):
    def _resolve(self, path):
        path = posixpath.normpath(path)
        if path == ".":
            path = "/"
        local = os.path.join(PAYLOAD_DIR, path.lstrip("/"))
        local = os.path.abspath(local)
        root = os.path.abspath(PAYLOAD_DIR)
        if not (local == root or local.startswith(root + os.sep)):
            return None
        return local

    def stat(self, path):
        local = self._resolve(path)
        if local is None or not os.path.exists(local):
            return paramiko.SFTP_NO_SUCH_FILE
        st = os.stat(local)
        attr = paramiko.SFTPAttributes()
        attr.st_size = st.st_size
        attr.st_mtime = int(st.st_mtime)
        attr.st_mode = st.st_mode
        attr.st_uid = st.st_uid
        attr.st_gid = st.st_gid
        return attr

    def lstat(self, path):
        return self.stat(path)

    def open(self, path, flags, attr):
        if flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC):
            return paramiko.SFTP_PERMISSION_DENIED
        local = self._resolve(path)
        if local is None or not os.path.isfile(local):
            return paramiko.SFTP_NO_SUCH_FILE

        class ReadOnlyFile(paramiko.SFTPHandle):
            def __init__(self, fname):
                super().__init__(flags)
                self._f = open(fname, "rb")

            def stat(self):
                return paramiko.SFTPAttributes()

            def read(self, offset, length):
                self._f.seek(offset)
                data = self._f.read(length)
                return data if data else paramiko.SFTP_EOF

            def close(self):
                self._f.close()
                return paramiko.SFTP_OK

        return ReadOnlyFile(local)

    def listdir(self, path):
        local = self._resolve(path)
        if local is None or not os.path.isdir(local):
            return paramiko.SFTP_NO_SUCH_FILE
        out = []
        for name in os.listdir(local):
            st = os.stat(os.path.join(local, name))
            attr = paramiko.SFTPAttributes()
            attr.filename = name
            attr.st_size = st.st_size
            attr.st_mtime = int(st.st_mtime)
            attr.st_mode = st.st_mode
            out.append(attr)
        return out

class SSHServer(paramiko.ServerInterface):
    def check_auth_password(self, username, password):
        if username == "ariaflow" and password:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"

    def check_channel_request(self, kind, chanid):
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    # Note: the default check_channel_subsystem_request handles the "sftp"
    # subsystem via the handler registered with set_subsystem_handler below.

_TRANSPORTS = []  # keep references so transports are not garbage-collected

def sftp_accept_loop():
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", SFTP_PORT))
    srv.listen(8)
    while True:
        client, _ = srv.accept()

        def handle():
            try:
                transport = paramiko.Transport(client)
                transport.add_server_key(HOST_KEY)
                transport.set_subsystem_handler("sftp", paramiko.SFTPServer, SFTPServerInterface)
                transport.start_server(server=SSHServer())
                _TRANSPORTS.append(transport)
            except Exception as e:
                print(f"[sftp] connection error: {e}", file=sys.stderr)
                try:
                    client.close()
                except Exception:
                    pass

        threading.Thread(target=handle, daemon=True).start()

sftp_thread = threading.Thread(target=sftp_accept_loop, daemon=True)
sftp_thread.start()

print(f"payload: {PAYLOAD} ({os.path.getsize(PAYLOAD)} bytes) sha256={DIGEST}", flush=True)
print(f"http={HTTP_PORT} throttle={THROTTLE_PORT} ftp={FTP_PORT} sftp={SFTP_PORT}", flush=True)

# Keep the process alive.
try:
    while True:
        time.sleep(3600)
except KeyboardInterrupt:
    pass
