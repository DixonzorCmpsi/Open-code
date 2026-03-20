// src/codegen/shared_js.rs
// Shared JavaScript code generation helpers used by both mcp.rs and runtime.rs.
// The _fetch variants use raw Node.js fetch() — zero npm dependencies.
// The SDK variants (used by mcp.rs) use @anthropic-ai/sdk.

/// Anthropic agent loop via raw fetch — zero npm dependencies.
/// Returns the agent's text output as a plain string (not an MCP response object).
/// Used by runtime.rs.
pub fn emit_agent_runner_anthropic_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: i64,
    temperature: f64,
    tools_filter: &str,
) -> String {
    format!(
        r#"async function {fn_name}(task) {{
  if (!process.env.ANTHROPIC_API_KEY) {{
    throw {{ code: "E-RT03", message: "ANTHROPIC_API_KEY not set\n  export ANTHROPIC_API_KEY=sk-ant-..." }};
  }}
  const agentTools = {tools_filter};
  const messages = [{{ role: "user", content: task }}];
  let steps = 0;

  while (steps < {max_steps}) {{
    steps++;
    const resp = await fetch("https://api.anthropic.com/v1/messages", {{
      method: "POST",
      headers: {{
        "x-api-key":         process.env.ANTHROPIC_API_KEY,
        "anthropic-version": "2023-06-01",
        "content-type":      "application/json",
      }},
      body: JSON.stringify({{
        model:      "{model}",
        system:     "{system_prompt}",
        messages,
        tools: agentTools.length > 0 ? agentTools.map(t => ({{
          name:         t.name,
          description:  t.description,
          input_schema: t.inputSchema,
        }})) : undefined,
        max_tokens:  4096,
        temperature: {temperature},
      }}),
    }});
    if (!resp.ok) {{
      const text = await resp.text();
      throw {{ code: "E-RUN04", message: `Anthropic API error ${{resp.status}}: ${{text}}` }};
    }}
    const data = await resp.json();

    if (data.stop_reason === "end_turn") {{
      return data.content.find(b => b.type === "text")?.text ?? "";
    }}

    if (data.stop_reason === "tool_use") {{
      const toolUseBlocks = data.content.filter(b => b.type === "tool_use");
      messages.push({{ role: "assistant", content: data.content }});
      const toolResults = [];
      for (const tu of toolUseBlocks) {{
        let resultContent;
        try {{
          resultContent = JSON.stringify(await callTool(tu.name, tu.input));
        }} catch (e) {{
          resultContent = `Error: ${{e.message ?? String(e)}}`;
        }}
        toolResults.push({{ type: "tool_result", tool_use_id: tu.id, content: resultContent }});
      }}
      messages.push({{ role: "user", content: toolResults }});
      continue;
    }}
    break;
  }}

  return `Agent {fn_name} reached max_steps ({max_steps}) without finishing.`;
}}"#,
        fn_name = fn_name,
        tools_filter = tools_filter,
        max_steps = max_steps,
        model = model,
        system_prompt = system_prompt,
        temperature = temperature,
    )
}

/// Ollama agent loop via raw fetch — zero npm dependencies.
/// Returns the agent's text output as a plain string.
/// Used by runtime.rs.
pub fn emit_agent_runner_ollama_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: i64,
    temperature: f64,
    tools_filter: &str,
) -> String {
    format!(
        r#"async function {fn_name}(task) {{
  const host = process.env.OLLAMA_HOST ?? "http://localhost:11434";
  const agentTools = {tools_filter};
  const messages = [
    {{ role: "system", content: "{system_prompt}" }},
    {{ role: "user",   content: task }},
  ];
  let steps = 0;

  while (steps < {max_steps}) {{
    steps++;
    const resp = await fetch(`${{host}}/v1/chat/completions`, {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify({{
        model:       "{model}",
        messages,
        tools: agentTools.length > 0 ? agentTools.map(t => ({{
          type: "function",
          function: {{ name: t.name, description: t.description, parameters: t.inputSchema }},
        }})) : undefined,
        stream:      false,
        temperature: {temperature},
      }}),
    }});
    if (!resp.ok) {{
      const text = await resp.text();
      throw {{ code: "E-RUN04", message: `Ollama error ${{resp.status}}: ${{text}}\n  hint: start Ollama with \`ollama serve\`` }};
    }}
    const data = await resp.json();
    const choice = data.choices?.[0];
    if (!choice) break;

    if (choice.finish_reason === "stop" || choice.finish_reason === "length") {{
      return choice.message?.content ?? "";
    }}

    if (choice.finish_reason === "tool_calls") {{
      const toolCalls = choice.message?.tool_calls ?? [];
      messages.push({{ role: "assistant", content: choice.message?.content ?? null, tool_calls: toolCalls }});
      for (const tc of toolCalls) {{
        let resultContent;
        try {{
          const tcArgs = typeof tc.function.arguments === "string"
            ? JSON.parse(tc.function.arguments)
            : tc.function.arguments;
          resultContent = JSON.stringify(await callTool(tc.function.name, tcArgs));
        }} catch (e) {{
          resultContent = `Error: ${{e.message ?? String(e)}}`;
        }}
        messages.push({{ role: "tool", tool_call_id: tc.id, content: resultContent }});
      }}
      continue;
    }}
    break;
  }}

  return `Agent {fn_name} reached max_steps ({max_steps}) without finishing.`;
}}"#,
        fn_name = fn_name,
        tools_filter = tools_filter,
        max_steps = max_steps,
        system_prompt = system_prompt,
        model = model,
        temperature = temperature,
    )
}
