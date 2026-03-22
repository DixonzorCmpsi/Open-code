"""
scripts/fetch_image.py — download a remote image to a local path
Called by claw tools with invoke: module("scripts/fetch_image").function("run")

No external dependencies — uses stdlib urllib only.
"""
from __future__ import annotations

import urllib.error
import urllib.parse
import urllib.request
import os
import sys
import hashlib
from pathlib import Path


# Known image magic byte sequences: (prefix_bytes, mime_type)
# We check the actual file content, not just the Content-Type header.
_MAGIC_SIGNATURES: list[tuple[bytes, str]] = [
    (b'\xff\xd8\xff', 'image/jpeg'),
    (b'\x89PNG\r\n\x1a\n', 'image/png'),
    (b'GIF87a', 'image/gif'),
    (b'GIF89a', 'image/gif'),
    (b'RIFF', 'image/webp'),       # followed by 4 bytes size then "WEBP"
    (b'<svg', 'image/svg+xml'),
    (b'<?xml', 'image/svg+xml'),   # SVG with XML declaration
    (b'BM', 'image/bmp'),
    (b'\x00\x00\x01\x00', 'image/x-icon'),
    (b'\x00\x00\x02\x00', 'image/x-icon'),
]

_ALLOWED_MIME_TYPES = {
    'image/jpeg', 'image/png', 'image/gif', 'image/webp',
    'image/svg+xml', 'image/bmp', 'image/tiff', 'image/avif',
    'image/x-icon',
}


def _detect_image_mime(data: bytes) -> str | None:
    """
    Validate that *data* is a known image format by checking magic bytes.
    Returns the detected MIME type, or None if not a recognised image format.
    """
    for magic, mime in _MAGIC_SIGNATURES:
        if data[:len(magic)] == magic:
            # Extra check for WEBP: bytes 8-11 must be b'WEBP'
            if mime == 'image/webp' and data[8:12] != b'WEBP':
                continue
            return mime
    return None


# Map MIME type → canonical extension
_MIME_EXT = {
    "image/jpeg": ".jpg",
    "image/png": ".png",
    "image/gif": ".gif",
    "image/webp": ".webp",
    "image/svg+xml": ".svg",
    "image/bmp": ".bmp",
    "image/tiff": ".tiff",
    "image/avif": ".avif",
}


def run(url: str, dest_dir: str = "~/Desktop") -> dict:
    """
    Download an image from *url* into *dest_dir*.

    The filename is derived from the URL path; if the URL has no extension,
    the MIME type is used. A content hash prefix is prepended to avoid
    collisions.

    Returns:
        {
            "url":        str,   # original URL
            "path":       str,   # absolute local path written
            "filename":   str,   # just the filename
            "mime_type":  str,   # e.g. "image/jpeg"
            "size_bytes": int,   # bytes downloaded
            "success":    bool
        }
    """
    dest = Path(os.path.expanduser(dest_dir)).resolve()
    dest.mkdir(parents=True, exist_ok=True)

    try:
        req = urllib.request.Request(
            url,
            headers={
                "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                              "AppleWebKit/537.36 (KHTML, like Gecko) "
                              "Chrome/122.0.0.0 Safari/537.36",
                "Accept": "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
            },
        )
        with urllib.request.urlopen(req, timeout=30) as resp:
            mime_type = resp.headers.get("Content-Type", "").split(";")[0].strip()
            data = resp.read()
    except (urllib.error.HTTPError, urllib.error.URLError, OSError) as exc:
        return {
            "url": url,
            "path": "",
            "filename": "",
            "mime_type": "",
            "size_bytes": 0,
            "success": False,
            "error": str(exc),
        }

    # Validate actual content via magic bytes — do not trust Content-Type header alone
    detected_mime = _detect_image_mime(data)
    if detected_mime is None:
        return {
            "url": url,
            "path": "",
            "filename": "",
            "mime_type": mime_type,
            "size_bytes": len(data),
            "success": False,
            "error": f"response is not a recognised image format (Content-Type: {mime_type!r}; first 16 bytes: {data[:16]!r})",
        }
    # Use magic-detected MIME (authoritative) over server-reported MIME
    mime_type = detected_mime

    # Determine filename
    url_path = urllib.parse.urlparse(url).path
    base_name = os.path.basename(url_path) or "image"
    stem, ext = os.path.splitext(base_name)

    if not ext or ext.lower() not in {".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".bmp", ".tiff", ".avif"}:
        ext = _MIME_EXT.get(mime_type, ".bin")

    # Content-hash prefix (first 8 hex chars) prevents collisions
    content_hash = hashlib.sha256(data).hexdigest()[:8]
    filename = f"{content_hash}-{stem}{ext}"
    out_path = dest / filename

    out_path.write_bytes(data)

    return {
        "url": url,
        "path": str(out_path),
        "filename": filename,
        "mime_type": mime_type,
        "size_bytes": len(data),
        "success": True,
    }


if __name__ == "__main__":
    import json
    if len(sys.argv) < 2:
        print("usage: fetch_image.py <url> [dest_dir]")
        sys.exit(1)
    target_url = sys.argv[1]
    destination = sys.argv[2] if len(sys.argv) > 2 else "~/Desktop"
    print(json.dumps(run(target_url, destination), indent=2))
