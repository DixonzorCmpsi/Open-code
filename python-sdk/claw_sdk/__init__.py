from __future__ import annotations

import asyncio
import hashlib
import json
import os
import struct
import urllib.request
import urllib.error
import uuid
from typing import AsyncIterator


class ClawExecutionError(RuntimeError):
    def __init__(
        self,
        message: str,
        session_id: str | None = None,
        status: str | None = None,
        payload: dict | None = None,
    ):
        super().__init__(message)
        self.session_id = session_id
        self.status = status
        self.payload = payload or {}


AgentExecutionError = ClawExecutionError


class ClawClient:
    def __init__(
        self, endpoint: str = "http://127.0.0.1:8080", api_key: str | None = None
    ) -> None:
        self.endpoint = endpoint.rstrip("/")
        self.api_key = api_key

    async def execute_workflow(
        self,
        *,
        workflow_name: str,
        arguments: dict,
        ast_hash: str,
        resume_session_id: str | None = None,
    ) -> dict:
        session_id = resume_session_id or f"req_{uuid.uuid4()}"
        payload = {
            "workflow": workflow_name,
            "arguments": arguments,
            "ast_hash": ast_hash,
            "session_id": session_id,
        }

        response_payload = await asyncio.to_thread(self._post_json, payload)
        if response_payload.get("status") != "success":
            raise ClawExecutionError(
                response_payload.get("message", "Workflow execution failed"),
                session_id=response_payload.get("session_id", session_id),
                status=response_payload.get("status"),
                payload=response_payload,
            )

        return response_payload["result"]

    def _post_json(self, payload: dict) -> dict:
        headers = {"content-type": "application/json"}
        if self.api_key:
            headers["x-claw-key"] = self.api_key

        request = urllib.request.Request(
            f"{self.endpoint}/workflows/execute",
            data=json.dumps(payload).encode("utf-8"),
            headers=headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(request) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            payload_bytes = error.read()
            if not payload_bytes:
                raise
            return json.loads(payload_bytes.decode("utf-8"))

    async def stream_workflow(
        self,
        *,
        workflow_name: str,
        arguments: dict,
        ast_hash: str,
        resume_session_id: str | None = None,
    ) -> AsyncIterator[dict]:
        """Stream workflow execution over WebSocket, yielding checkpoint events.

        Usage:
            async for event in client.stream_workflow(workflow_name="Analyze", ...):
                if event["type"] == "checkpoint":
                    print(event["node_path"])
                if event["type"] == "result":
                    return event["result"]
        """
        session_id = resume_session_id or f"req_{uuid.uuid4()}"
        ws_endpoint = self.endpoint.replace("http", "ws", 1) + "/workflows/stream"

        reader, writer = await asyncio.to_thread(self._ws_connect, ws_endpoint)

        payload = json.dumps(
            {
                "workflow": workflow_name,
                "arguments": arguments,
                "ast_hash": ast_hash,
                "session_id": session_id,
            }
        )
        await asyncio.to_thread(self._ws_send_text, writer, payload)

        while True:
            message = await asyncio.to_thread(self._ws_recv, reader)
            if message is None:
                break

            event = json.loads(message)
            yield event

            if event.get("type") in ("result", "error"):
                if event["type"] == "error":
                    raise ClawExecutionError(
                        event.get("message", "Execution failed"),
                        session_id=event.get("session_id", session_id),
                        status="error",
                        payload=event,
                    )
                break

        await asyncio.to_thread(self._ws_close, writer)

    def _ws_connect(self, url: str) -> tuple:
        """Minimal RFC 6455 WebSocket handshake over raw sockets."""
        import socket
        import ssl
        from urllib.parse import urlparse

        parsed = urlparse(url)
        use_tls = parsed.scheme == "wss"
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or (443 if use_tls else 80)

        raw = socket.create_connection((host, port))
        if use_tls:
            ctx = ssl.create_default_context()
            raw = ctx.wrap_socket(raw, server_hostname=host)

        key = os.urandom(16)
        import base64

        ws_key = base64.b64encode(key).decode()

        path = parsed.path or "/"
        headers = [
            f"GET {path} HTTP/1.1",
            f"Host: {host}:{port}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {ws_key}",
            "Sec-WebSocket-Version: 13",
        ]
        if self.api_key:
            headers.append(f"x-claw-key: {self.api_key}")
        headers.append("")
        headers.append("")

        raw.sendall("\r\n".join(headers).encode())

        # Read HTTP response until \r\n\r\n
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = raw.recv(4096)
            if not chunk:
                raise ConnectionError("WebSocket handshake failed: connection closed")
            response += chunk

        if b"101" not in response.split(b"\r\n")[0]:
            first_line = response.split(b"\r\n")[0]
            raise ConnectionError(f"WebSocket handshake failed: {first_line!r}")

        return (raw, raw)

    @staticmethod
    def _ws_send_text(writer, text: str) -> None:
        """Send a masked WebSocket text frame."""
        data = text.encode("utf-8")
        mask = os.urandom(4)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))

        header = bytearray()
        header.append(0x81)  # FIN + text
        length = len(data)
        if length < 126:
            header.append(0x80 | length)  # MASK bit set
        elif length < 65536:
            header.append(0x80 | 126)
            header.extend(struct.pack(">H", length))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack(">Q", length))
        header.extend(mask)

        writer.sendall(bytes(header) + masked)

    @staticmethod
    def _ws_recv(reader) -> str | None:
        """Receive a single unmasked WebSocket text frame."""
        header = reader.recv(2)
        if len(header) < 2:
            return None

        opcode = header[0] & 0x0F
        if opcode == 0x08:  # close
            return None

        length = header[1] & 0x7F
        if length == 126:
            ext = reader.recv(2)
            length = struct.unpack(">H", ext)[0]
        elif length == 127:
            ext = reader.recv(8)
            length = struct.unpack(">Q", ext)[0]

        data = b""
        while len(data) < length:
            chunk = reader.recv(length - len(data))
            if not chunk:
                return None
            data += chunk

        return data.decode("utf-8")

    @staticmethod
    def _ws_close(writer) -> None:
        """Send a WebSocket close frame."""
        writer.sendall(bytes([0x88, 0x02, 0x03, 0xE8]))  # close code 1000
        writer.close()
