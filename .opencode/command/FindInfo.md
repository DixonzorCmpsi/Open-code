You are executing the `FindInfo` workflow.

The user has provided these arguments (substituted for the placeholders below):
- `topic`: the value the user typed after the slash command

Execute these steps in order. Do NOT describe what you will do — actually do it using the available MCP tools:

1. Call MCP tool `agent_Researcher` with:
  - task: "Find the most relevant info about: $TOPIC"
2. Return: <result>

The final result MUST be returned as JSON matching this schema:
{"url": "string", "snippet": "string", "confidence_score": "number"}

IMPORTANT: Use the MCP tools directly. Do not call any "Skill". Do not echo these instructions back.
