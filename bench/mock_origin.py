#!/usr/bin/env python3
"""
Benchmark Mock Origin Server
Serves static corpus images, simulates slow and failing upstreams,
and tracks request counts for single-flight coalescing verification.
"""

import http.server
import os
import ssl
import subprocess
import sys
import time
import urllib.parse

CORPUS_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "benchmarks", "corpus"))
if not os.path.isdir(CORPUS_DIR):
    if os.path.isdir("/benchmarks/corpus"):
        CORPUS_DIR = "/benchmarks/corpus"
FETCH_COUNT = 0

class MockOriginHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        # Suppress noisy standard request logs during high concurrency benchmarks
        pass

    def do_GET(self):
        global FETCH_COUNT
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        query = urllib.parse.parse_qs(parsed.query)

        # Metrics/Stats endpoint
        if path == "/stats" or path == "/metrics":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(f"origin_fetches_total {FETCH_COUNT}\n".encode("utf-8"))
            return

        if path == "/stats/reset":
            FETCH_COUNT = 0
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"reset ok\n")
            return

        # Failing origin simulation
        if path.startswith("/fail/"):
            FETCH_COUNT += 1
            self.send_response(500)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"Internal Server Error: Mock Origin Simulated Failure\n")
            return

        # Slow origin simulation
        if path.startswith("/slow/"):
            FETCH_COUNT += 1
            delay = float(query.get("delay", [2.0])[0])
            time.sleep(delay)
            filename = os.path.basename(path)
            self.serve_file(filename)
            return

        # Normal corpus serve
        if path.startswith("/corpus/"):
            FETCH_COUNT += 1
            filename = os.path.basename(path)
            self.serve_file(filename)
            return

        self.send_response(404)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(b"Not Found\n")

    def serve_file(self, filename):
        file_path = os.path.join(CORPUS_DIR, filename)
        if not os.path.isfile(file_path):
            self.send_response(404)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"Corpus file not found\n")
            return

        ext = os.path.splitext(filename)[1].lower()
        content_type = "application/octet-stream"
        if ext in (".jpg", ".jpeg"):
            content_type = "image/jpeg"
        elif ext == ".png":
            content_type = "image/png"
        elif ext == ".webp":
            content_type = "image/webp"

        with open(file_path, "rb") as f:
            data = f.read()

        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "public, max-age=3600")
        self.end_headers()
        self.wfile.write(data)

def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    use_tls = "--tls" in sys.argv

    server = http.server.ThreadingHTTPServer(("0.0.0.0", port), MockOriginHandler)

    if use_tls:
        cert_dir = os.path.join(os.path.dirname(__file__), "certs")
        os.makedirs(cert_dir, exist_ok=True)
        ca_file = os.path.join(cert_dir, "ca.crt")
        ca_key_file = os.path.join(cert_dir, "ca.key")
        cert_file = os.path.join(cert_dir, "origin.crt")
        key_file = os.path.join(cert_dir, "origin.key")

        if not os.path.exists(ca_file) or not os.path.exists(cert_file) or not os.path.exists(key_file):
            ext_file = os.path.join(cert_dir, "origin.ext")
            with open(ext_file, "w") as f:
                f.write("basicConstraints=CA:FALSE\nsubjectAltName=DNS:origin,DNS:localhost,IP:127.0.0.1\n")
            csr_file = os.path.join(cert_dir, "origin.csr")
            subprocess.run([
                "openssl", "req", "-x509", "-newkey", "rsa:2048",
                "-keyout", ca_key_file, "-out", ca_file,
                "-days", "3650", "-nodes",
                "-subj", "/CN=BenchmarkCA/O=Benchmark/C=US"
            ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run([
                "openssl", "req", "-newkey", "rsa:2048",
                "-keyout", key_file, "-out", csr_file,
                "-nodes", "-subj", "/CN=origin/O=Benchmark/C=US"
            ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run([
                "openssl", "x509", "-req", "-in", csr_file,
                "-CA", ca_file, "-CAkey", ca_key_file, "-CAcreateserial",
                "-out", cert_file, "-days", "3650", "-extfile", ext_file
            ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for temp in [csr_file, ext_file]:
                if os.path.exists(temp):
                    os.remove(temp)

        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(certfile=cert_file, keyfile=key_file)
        server.socket = ctx.wrap_socket(server.socket, server_side=True)
        print(f"Mock Origin HTTPS Server listening on https://0.0.0.0:{port}...")
    else:
        print(f"Mock Origin HTTP Server listening on http://0.0.0.0:{port}...")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass

if __name__ == "__main__":
    main()
