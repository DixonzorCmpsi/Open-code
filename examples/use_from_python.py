#!/usr/bin/env python3
"""
Example: calling a .claw workflow from Python — just like BAML.

1. Build the claw file:
   claw build --lang python examples/research_report.claw

2. Install deps:
   pip install pydantic httpx

3. Run this script:
   ANTHROPIC_API_KEY=sk-ant-... python examples/use_from_python.py
"""
import asyncio
import sys
import os

# Add project root to path so we can import generated.claw
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from generated.claw import research_topic  # type: ignore


async def main():
    topic = sys.argv[1] if len(sys.argv) > 1 else "quantum computing"

    print(f"Researching: {topic}", flush=True)

    # Call the workflow exactly like a BAML function
    report = await research_topic(topic=topic)

    print(f"\nTopic:       {report.topic}")
    print(f"Summary:     {report.summary[:120]}...")
    print(f"Key Points:  {report.key_points[:80]}...")
    print(f"\nReport saved to ~/Documents/research/{topic}.md")


if __name__ == "__main__":
    asyncio.run(main())
