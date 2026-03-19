import asyncio
from claw_sdk import ClawClient

# Import the dynamically generated Pydantic wrappers from your build step
from generated.claw import analyze_company, ResearchReport


# Mock client for testing
class MockClawClient:
    async def execute_workflow(
        self,
        *,
        workflow_name: str,
        arguments: dict,
        ast_hash: str,
        resume_session_id: str | None = None,
    ) -> dict:
        # Return a mock result that matches the expected schema for AnalyzeCompany workflow
        if workflow_name == "AnalyzeCompany":
            return {
                "company_name": "Apple Inc.",
                "confidence": 0.95,
                "summary": "Apple releases new XR headset.",
                "tags": ["hardware", "xr"],
            }
        else:
            # For other workflows, return a generic result
            return {"workflow": workflow_name, "arguments": arguments}


async def main():
    # 1. Initialize the Client to point at the local Claw Gateway
    # Using mock client to avoid gateway/LLM dependencies
    client = MockClawClient()

    print("Executing workflow with mock client...")

    # 2. Call the execution wrapper.
    report: ResearchReport = await analyze_company(company="Apple Inc.", client=client)

    # 3. Deterministic output! Guaranteed to match your schema.
    print(f"Company: {report.company_name}")
    print(f"Confidence: {report.confidence}%")
    print(f"Summary: {report.summary}")
    print(f"Tags: {report.tags}")


if __name__ == "__main__":
    asyncio.run(main())
