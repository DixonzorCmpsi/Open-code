import process from "node:process";

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
});
process.stdin.on("end", () => {
  const payload = input.trim() ? JSON.parse(input) : {};
  process.stdout.write(
    JSON.stringify({
      runtime: "typescript",
      payload
    })
  );
});
