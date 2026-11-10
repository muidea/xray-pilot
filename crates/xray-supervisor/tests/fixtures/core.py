#!/usr/bin/python3
"""Deterministic child-process fixture. Never packaged with the application."""
import json
import select
import socket
import sys

if sys.argv[1] == "version":
    print("Xray deterministic-test-core")
    sys.exit(0)
config = json.load(open(sys.argv[sys.argv.index("-config") + 1], encoding="utf-8"))
if config.get("fixtureReject"):
    sys.exit(23)
if "-test" in sys.argv:
    sys.exit(0)
if config.get("fixtureFailStart"):
    sys.exit(24)
listeners = []
for inbound in config.get("inbounds", []):
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((inbound["listen"], inbound["port"]))
    server.listen()
    listeners.append(server)
while True:
    for server in select.select(listeners, [], [], 1)[0]:
        client, _ = server.accept()
        with client:
            client.recv(4096)
            client.sendall(b"HTTP/1.1 200 Connection established\r\nContent-Length: 0\r\n\r\n")
