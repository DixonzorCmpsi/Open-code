async function run() {
  const hash = "01f2bd7cd0126801c910ffa89f97b9878ad3eb960f1d2a6cd0c95528f98393fa";
  try {
    const res = await fetch("http://localhost:8080/workflows/execute", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        workflow: "AnalyzeCompetitors",
        arguments: { company: "Apple" },
        ast_hash: hash,
        session_id: "demo-session-88"
      })
    });
    
    console.log("STATUS:", res.status);
    const data = await res.json();
    console.log("RESPONSE:", JSON.stringify(data, null, 2));
  } catch (err) {
    console.error("ERROR:", err.message);
  }
}

run();
