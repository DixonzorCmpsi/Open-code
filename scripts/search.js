// stub search implementation — returns mock SearchResult
export async function run(query) {
  const slug = String(query).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "search";
  return {
    url: `https://${slug}.example.com`,
    snippet: `Stub result for: ${query}`,
    confidence_score: 0.9
  };
}
