"""
scripts/search.py — web search via DuckDuckGo (no API key required)
Called by claw tools with invoke: module("scripts/search").function("run")

Strategy (in order until a non-empty URL is found):
  1. DDG Instant Answer API — AbstractURL + Abstract (great for named entities)
  2. DDG Instant Answer API — Results[]
  3. DDG Instant Answer API — RelatedTopics[] (recursive, handles nested Topics)
  4. DDG HTML scrape — parses the regular search results page (product queries, etc.)
"""
from __future__ import annotations

import json
import re
import urllib.error
import urllib.parse
import urllib.request


_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/122.0.0.0 Safari/537.36"
    ),
    "Accept-Language": "en-US,en;q=0.9",
}


# ── Tier 1 & 2 & 3: DDG Instant Answer API ───────────────────────────────────

def _ddg_api(query: str) -> tuple[str, str]:
    """
    Call the DDG Instant Answer JSON API.
    Returns (url, snippet) — both may be empty strings.
    """
    params = urllib.parse.urlencode({
        "q": query,
        "format": "json",
        "no_html": "1",
        "skip_disambig": "1",
    })
    req = urllib.request.Request(
        f"https://api.duckduckgo.com/?{params}",
        headers=_HEADERS,
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data: dict = json.loads(resp.read().decode())
    except (urllib.error.URLError, OSError):
        return "", ""

    # Tier 1: AbstractURL (Wikipedia-style entities)
    url = data.get("AbstractURL") or ""
    snippet = data.get("Abstract") or ""
    if url:
        return url, snippet

    # Tier 2: explicit Results[]
    for result in data.get("Results") or []:
        url = result.get("FirstURL", "")
        snippet = result.get("Text", "")
        if url:
            return url, snippet

    # Tier 3: RelatedTopics[] — can be nested under "Topics" sub-key
    def _mine_topics(topics: list) -> tuple[str, str]:
        for topic in topics:
            # Nested group (e.g. {"Name": "...", "Topics": [...]})
            if "Topics" in topic:
                result = _mine_topics(topic["Topics"])
                if result[0]:
                    return result
            else:
                u = topic.get("FirstURL", "")
                s = topic.get("Text", "")
                if u:
                    return u, s
        return "", ""

    url, snippet = _mine_topics(data.get("RelatedTopics") or [])
    return url, snippet


# ── Tier 4: DDG HTML scrape ───────────────────────────────────────────────────

def _ddg_html(query: str) -> tuple[str, str]:
    """
    Scrape DuckDuckGo's regular HTML results page.
    Works for product queries and anything the Instant Answer API misses.
    Returns (url, snippet) — both may be empty strings.
    """
    params = urllib.parse.urlencode({"q": query, "ia": "web"})
    req = urllib.request.Request(
        f"https://html.duckduckgo.com/html/?{params}",
        headers={**_HEADERS, "Accept": "text/html"},
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            html = resp.read().decode(errors="replace")
    except (urllib.error.URLError, OSError):
        return "", ""

    # Extract result URLs from DuckDuckGo's redirect links:
    # <a class="result__url" href="//duckduckgo.com/l/?uddg=<encoded_url>&...">
    # or direct hrefs in result__a anchors
    url = ""
    snippet = ""

    # Try result__a first (direct link or redirect)
    for href in re.findall(r'class="result__a"[^>]*href="([^"]+)"', html):
        if href.startswith("//duckduckgo.com/l/"):
            # Decode the uddg parameter
            qs = urllib.parse.urlparse("https:" + href).query
            decoded = urllib.parse.parse_qs(qs).get("uddg", [""])[0]
            if decoded and not decoded.startswith("https://duckduckgo.com"):
                url = decoded
                break
        elif href.startswith("http"):
            url = href
            break

    # Extract first snippet
    snip_match = re.search(r'class="result__snippet"[^>]*>(.*?)</(?:a|span)>', html, re.S)
    if snip_match:
        snippet = re.sub(r"<[^>]+>", "", snip_match.group(1)).strip()

    return url, snippet


# ── Public API ────────────────────────────────────────────────────────────────

def run(query: str) -> dict:
    """
    Search DuckDuckGo and return the best result found.

    Returns:
        {
            "url":              str,   # top result URL (may be empty on total failure)
            "snippet":          str,   # description or summary
            "confidence_score": float  # 0.4 – 0.9 based on result quality
        }
    """
    # Try API tiers first (fast, no scraping)
    url, snippet = _ddg_api(query)

    # Fall back to HTML scraping if API returned no URL
    if not url:
        url, snippet = _ddg_html(query)

    confidence = 0.9 if (url and snippet) else (0.6 if url else 0.4)

    return {
        "url": url,
        "snippet": snippet or f"No result found for: {query}",
        "confidence_score": confidence,
    }


if __name__ == "__main__":
    import sys
    q = " ".join(sys.argv[1:]) if len(sys.argv) > 1 else "rust programming language"
    print(json.dumps(run(q), indent=2))
