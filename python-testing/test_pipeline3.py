import asyncio
from claw_sdk import ClawClient

# Import from the newly compiled file
from generated.claw import email_last_watched_video, EmailResult

async def main():
    client = ClawClient(endpoint="http://localhost:8080")
    print("Running YouTube Email Assistant...\n")
    
    # Execute the workflow
    result: EmailResult = await email_last_watched_video(
        email_address="dixonfzor@gmail.com",
        client=client
    )
    
    print("--- Output Received ---")
    print(f"Success: {result.success}")
    print(f"Video Link: {result.video_link}")
    print(f"Summary: {result.summary}")

if __name__ == "__main__":
    asyncio.run(main())
