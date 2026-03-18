import json
import sys
import time


def main() -> None:
    raw = sys.stdin.read().strip()
    payload = json.loads(raw) if raw else {}
    if "sleep" in payload:
        time.sleep(payload["sleep"])
    sys.stdout.write(
        json.dumps(
            {
                "runtime": "python",
                "payload": payload,
            }
        )
    )


if __name__ == "__main__":
    main()
