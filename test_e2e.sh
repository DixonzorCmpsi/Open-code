#!/bin/bash
set -e

# 1. Clean up
rm -rf test-project
mkdir test-project
cd test-project

# 2. Run claw init (assuming claw is in path or calling via cargo run)
# For the test, we'll use the current directory's binary
CLAW_BIN="../target/debug/claw"
if [ ! -f "$CLAW_BIN" ]; then
    echo "Building claw..."
    cargo build --bin claw
fi

echo "Running: $CLAW_BIN init"
$CLAW_BIN init

# 3. Validate scaffolded files
[ -f "example.claw" ] || (echo "Missing example.claw" && exit 1)
[ -f "claw.json" ] || (echo "Missing claw.json" && exit 1)
[ -f "package.json" ] || (echo "Missing package.json" && exit 1)
[ -f "scripts/search.js" ] || (echo "Missing scripts/search.js" && exit 1)

# 4. Run claw build
echo "Running: $CLAW_BIN build"
$CLAW_BIN build

# 5. Validate output Content
echo "Validating opencode.json..."
node -e "
const fs = require('fs');
const config = JSON.parse(fs.readFileSync('opencode.json', 'utf8'));
if (!config.agents || !config.agents.coder || config.agents.coder.model !== 'claude-4-sonnet') {
    console.error('Invalid agents config', config.agents);
    process.exit(1);
}
if (!config.mcpServers || !config.mcpServers['claw-tools'] || config.mcpServers['claw-tools'].type !== 'stdio') {
    console.error('Invalid mcpServers config', config.mcpServers);
    process.exit(1);
}
if (!config.contextPaths || !config.contextPaths.includes('generated/claw-context.md')) {
    console.error('Invalid contextPaths', config.contextPaths);
    process.exit(1);
}
"

echo "Validating .opencode/commands/FindInfo.md..."
grep "\$TOPIC" .opencode/commands/FindInfo.md || (echo "Missing \$TOPIC in command file" && exit 1)
grep "agent_Researcher" .opencode/commands/FindInfo.md || (echo "Missing agent_Researcher in command file" && exit 1)

echo "Validating generated/mcp-server.js..."
[ -f "generated/mcp-server.js" ] || (echo "Missing mcp-server.js" && exit 1)
grep "agent_Researcher" generated/mcp-server.js || (echo "Missing agent_Researcher in MCP server" && exit 1)
# Validate bash syntax of the generated file (basic check)
node -c generated/mcp-server.js || (echo "MCP server has JS syntax errors" && exit 1)

echo "Validating .opencode/agents/ does NOT exist..."
[ ! -d ".opencode/agents" ] || (echo ".opencode/agents still exists" && exit 1)

echo "✓ E2E Smoke Test Passed!"
