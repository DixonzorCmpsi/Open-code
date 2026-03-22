"""
scripts/fetch_page.py — three-tier page fetcher (static → dynamic → stealth)
Called by claw tools with invoke: module("scripts/fetch_page").function("run")

Tier 1: urllib (fast, no JS, works for most pages)
Tier 2: playwright (headless Chromium, renders JS — requires: pip install playwright && playwright install chromium)
Tier 3: scrapling StealthyFetcher (Cloudflare bypass — requires: pip install scrapling && scrapling install)

Falls back gracefully if optional deps are not installed.
"""
from __future__ import annotations

import urllib.request
import urllib.parse
import urllib.error
import logging
import re
import sys
from typing import Optional

logger = logging.getLogger(__name__)


def _tier1_urllib(url: str, timeout: int = 15) -> Optional[dict]:
    """Static HTTP fetch via urllib. Fast, no JS rendering."""
    try:
        req = urllib.request.Request(
            url,
            headers={
                "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                              "AppleWebKit/537.36 (KHTML, like Gecko) "
                              "Chrome/122.0.0.0 Safari/537.36",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                "Accept-Language": "en-US,en;q=0.5",
            },
        )
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            content_type = resp.headers.get("Content-Type", "")
            raw = resp.read()
            if "text" in content_type or "html" in content_type:
                html = raw.decode(errors="replace")
                return {"html": html, "tier": "urllib", "status": resp.status}
            else:
                return None  # binary content — not a page
    except (urllib.error.HTTPError, urllib.error.URLError, OSError) as exc:
        logger.debug("tier1 urllib failed for %s: %s", url, exc)
        return None


def _tier2_playwright(url: str, timeout: int = 20) -> Optional[dict]:
    """Dynamic JS-rendered fetch via Playwright headless Chromium."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        return None

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page(
                user_agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                           "AppleWebKit/537.36 (KHTML, like Gecko) "
                           "Chrome/122.0.0.0 Safari/537.36"
            )
            page.goto(url, timeout=timeout * 1000, wait_until="domcontentloaded")
            html = page.content()
            browser.close()
            return {"html": html, "tier": "playwright", "status": 200}
    except Exception as exc:
        logger.debug("tier2 playwright failed for %s: %s", url, exc)
        return None


def _tier3_scrapling(url: str) -> Optional[dict]:
    """Stealth fetch via Scrapling (bypasses Cloudflare, bot detection)."""
    try:
        from scrapling import StealthyFetcher
    except ImportError:
        return None

    try:
        fetcher = StealthyFetcher()
        page = fetcher.fetch(url)
        return {"html": page.html_content, "tier": "scrapling", "status": 200}
    except Exception as exc:
        logger.debug("tier3 scrapling failed for %s: %s", url, exc)
        return None


def _extract_text(html: str) -> str:
    """Strip tags, collapse whitespace — rough but dependency-free."""
    # Remove script/style blocks
    html = re.sub(r"<(script|style)[^>]*>.*?</(script|style)>", "", html, flags=re.S | re.I)
    # Strip tags
    text = re.sub(r"<[^>]+>", " ", html)
    # Collapse whitespace
    text = re.sub(r"\s+", " ", text).strip()
    return text[:8000]  # cap at 8 000 chars


def run(url: str) -> dict:
    """
    Fetch a web page and return its text content.

    Returns:
        {
            "url": str,
            "text": str,        # plain text extracted from HTML
            "title": str,       # <title> tag contents or ""
            "tier": str,        # "urllib" | "playwright" | "scrapling"
            "success": bool
        }
    """
    result = (
        _tier1_urllib(url)
        or _tier2_playwright(url)
        or _tier3_scrapling(url)
    )

    if result is None:
        return {
            "url": url,
            "text": f"Failed to fetch page: {url}",
            "title": "",
            "tier": "none",
            "success": False,
        }

    html = result["html"]

    # Extract <title>
    title_match = re.search(r"<title[^>]*>(.*?)</title>", html, re.S | re.I)
    title = title_match.group(1).strip() if title_match else ""

    text = _extract_text(html)

    return {
        "url": url,
        "text": text,
        "title": title,
        "tier": result["tier"],
        "success": True,
    }


if __name__ == "__main__":
    import json
    target = sys.argv[1] if len(sys.argv) > 1 else "https://example.com"
    print(json.dumps(run(target), indent=2))
