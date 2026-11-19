#!/usr/bin/env python3
"""A fake api.github.com for the Exact port's parity drives, answered from github_fake.py:
POST /graphql, GET /user and GET /notifications. `python3 api.py [port]` (default 8787).
MEAT_PROXY_FAKE_GH=signed-out answers every request 401, as GitHub does for a revoked token."""
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
from github_fake import NOW, VIEWER, graphql  # noqa: E402


class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body, headers=()):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "authorization, content-type, if-modified-since")
        for k, v in headers:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def signed_out(self):
        if os.environ.get("MEAT_PROXY_FAKE_GH") == "signed-out" or not self.headers.get("Authorization"):
            self.reply(401, {"message": "Bad credentials"})
            return True
        return False

    def do_OPTIONS(self):
        self.reply(204, {})

    def do_GET(self):
        if self.signed_out():
            return
        if self.path.startswith("/user"):
            return self.reply(200, {"login": VIEWER})
        if self.path.startswith("/notifications"):
            stamp = NOW.strftime("%a, %d %b %Y %H:%M:%S GMT")
            return self.reply(200, [], [("Last-Modified", stamp), ("X-Poll-Interval", "60")])
        self.reply(404, {"message": "Not Found"})

    def do_POST(self):
        if self.signed_out():
            return
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        if self.path.startswith("/graphql"):
            return self.reply(200, graphql(body.get("query", "")))
        self.reply(404, {"message": "Not Found"})

    def log_message(self, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8787), Handler).serve_forever()
