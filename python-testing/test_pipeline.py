import asyncio
from claw_sdk import ClawClient

# Import the dynamically generated Pydantic wrappers from your build step
from generated.claw import analyze_company, ResearchReport

async def main():
    # 1. Initialize the Client to point at the local Claw Gateway
    client = ClawClient(endpoint="http://localhost:8080")
    
    print("Executing workflow...")
    
    # 2. Call the execution wrapper. 
    report: ResearchReport = await analyze_company(
        company="Apple Inc.",
        client=client
    )
    
    # 3. Deterministic output! Guaranteed to match your schema.
    print(f"Company: {report.company_name}")
    print(f"Confidence: {report.confidence}%")
    print(f"Summary: {report.summary}")
    print(f"Tags: {report.tags}")

if __name__ == "__main__":
    asyncio.run(main())
