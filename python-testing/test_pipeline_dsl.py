import asyncio
from claw_sdk import ClawClient
from generated.claw import dsl_pipeline_test, EmailResult

async def main():
    client = ClawClient(endpoint="http://localhost:8080")
    print("Testing Native OpenClaw DSL Multi-Agent Orchestration...\n")
    
    # Fire off our pure-DSL workflow
    result: EmailResult = await dsl_pipeline_test(
        email_address="dixonfzor@gmail.com",
        client=client
    )
    
    print("--- 2nd Agent Final Output ---")
    print(f"Success: {result.success}")
    print(f"Video Link Context Passed Natively: {result.video_link}")
    print(f"Summary: {result.summary}")

if __name__ == "__main__":
    asyncio.run(main())
