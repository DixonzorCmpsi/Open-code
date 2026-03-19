import asyncio
from claw_sdk import ClawClient
from generated.claw import retrieve_history_and_email, Evidence

async def main():
    client = ClawClient(endpoint="http://localhost:8080")
    print("Executing Native .claw DSL Orchestration live against Chrome Profile!")
    
    result = await retrieve_history_and_email(
        email_account="dixonfzor@gmail.com",
        client=client
    )
    
    print("\n--- Output Received Natively from Gateway ---")
    print(f"Success: {result.success}")
    print(f"Extracted URL: {result.extracted_link}")
    
    # We don't even need to send the email manually!
    # The .claw script internally called BrowserNavigate(mailto:), physically popping the Windows Mail app!
    print("Action complete. Check your primary mail application window!")

if __name__ == "__main__":
    asyncio.run(main())
