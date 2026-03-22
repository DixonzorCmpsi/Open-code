# Spec 50 — Artifact Output Formats

**Status:** DRAFT
**Depends on:** Spec 03 (Grammar), Spec 32 (Synthesis Pipeline), Spec 39 (Runtime-First Architecture)
**Implements:** `artifact { format = "..." }` block in workflow declarations

---

## 1. Overview

The `artifact {}` block in a workflow declaration declares what file the runtime should write when the workflow completes. The `format` field controls how the return value is serialised to disk.

```claw
workflow MyWorkflow() -> MyType {
    artifact {
        format = "pptx"              // output format token
        path   = "~/Desktop/out.pptx" // supports ~ and ${arg} expansion
    }
    ...
}
```

The format token is a **string literal** validated at compile time by the semantic analyser. Unknown tokens produce error `E-ART01`.

---

## 2. Supported Format Tokens

| Token | File Extension | Requires | Zero-dep |
|-------|---------------|----------|----------|
| `json` | `.json` | — | ✓ |
| `markdown` / `md` | `.md` | — | ✓ |
| `html` | `.html` | — | ✓ |
| `slides` | `.html` | — | ✓ |
| `csv` | `.csv` | — | ✓ |
| `pptx` | `.pptx` | `pptxgenjs` npm package | ✗ |
| `text` | `.txt` | — | ✓ |

Aliases: `markdown` and `md` are identical.

---

## 3. Convention-Based Field Mapping

Formats other than `json` and `csv` use **convention-based field detection** to map the return value's fields to document structure. The resolver looks for fields **by name** in order:

| Role | Looked-up field names (first match wins) |
|------|------------------------------------------|
| **title** | `title`, `name`, `heading` |
| **subtitle / summary** | `summary`, `description`, `abstract`, `subtitle` |
| **body sections** | `sections`, `content`, `body`, `paragraphs`, `slides`, `items`, `chapters` |
| **tags / labels** | `tags`, `labels`, `keywords`, `categories` |
| **author** | `author`, `by`, `creator` |

When a body-sections field contains an **array of strings**, each string becomes one section/slide/row. When it is a **plain string**, it is used as a single body block.

If none of the above fields are found, the entire return value is JSON-pretty-printed as the body.

---

## 4. Format Specifications

### 4.1 `json`

`JSON.stringify(result, null, 2)` written as UTF-8.

### 4.2 `markdown` / `md`

```
# <title>

<summary>

## <sections[0]>

## <sections[1]>

...

---
Tags: <tags.join(", ")>
```

### 4.3 `html`

A self-contained HTML5 page with embedded minimal CSS. Structure:

- `<h1>` — title
- `<p class="summary">` — summary
- `<section>` per body-section entry
- `<footer>` — tags

No external resources. Suitable for opening directly in a browser.

### 4.4 `slides`

[Reveal.js](https://revealjs.com/) self-contained HTML presentation, CDN-hosted assets (requires internet at view time, not at build time).

- Slide 1: title + summary
- Slides 2-N: one `<section>` per body-section entry
- Last slide: tags

The output is a single `.html` file. Open in any browser. No npm install required.

### 4.5 `csv`

Flat CSV. Behaviour depends on the return type shape:

- **Object with array-of-string field** (e.g. `sections: list<string>`): one row per string, header = field name.
- **Object**: single row, headers = field names, values = field values (arrays serialised as JSON).
- **Primitive**: single-column CSV.

RFC 4180 quoting: values containing commas, newlines, or `"` are wrapped in double-quotes with internal `"` escaped as `""`.

### 4.6 `pptx`

PowerPoint presentation via `pptxgenjs` (`npm install pptxgenjs`).

Slide layout:
- Slide 1: title (large) + summary (subtitle)
- Slides 2-N: title bar with section index, body text from each sections array entry
- Last slide: "Thank you" + tags as bullet points

If `pptxgenjs` is not installed, the runtime emits error `E-ART02` with install instructions and exits 4.

### 4.7 `text`

Plain text. Uses the `summary` or `content` field if present, otherwise all string fields joined with newlines.

---

## 5. Path Expansion

The `path` value undergoes two expansion passes before writing:

1. **Home dir**: leading `~` → `os.homedir()`
2. **Arg interpolation**: `${argName}` → the workflow argument value for `argName`

The parent directory is created recursively before writing (equivalent to `mkdir -p`).

---

## 6. Error Codes

| Code | Meaning | Exit |
|------|---------|------|
| `E-ART01` | Unknown format token (compile-time) | 2 |
| `E-ART02` | `pptxgenjs` not installed (runtime) | 4 |
| `E-ART03` | Cannot write artifact to path (runtime) | 4 |

---

## 7. Semantic Validation (Compile-Time)

The semantic analyser validates:

1. `format` is one of the known tokens — emits `E-ART01` if unknown.
2. `path` is a non-empty string literal — emits `E-ART03` warning if the extension doesn't match the format (e.g. `format = "pptx"` but path ends in `.json`). Not an error, just a warning.

---

## 8. Grammar (no change)

The grammar rule for `artifact_decl` in Spec 03 is unchanged:

```pest
artifact_decl = { "artifact" ~ "{" ~ artifact_prop+ ~ "}" }
artifact_prop = { ("format" | "path") ~ "=" ~ string_literal }
```

The set of valid `format` values is enforced semantically, not syntactically.

---

## 9. Implementation Checklist

- [ ] `src/semantic/mod.rs` — validate format token against `KNOWN_FORMATS` list, emit `E-ART01`
- [ ] `src/codegen/runtime.rs` — replace stub `saveArtifact` with full multi-format renderer
- [ ] `package.json` — add `pptxgenjs` as `optionalDependencies`
- [ ] `specs/03-Grammar.md` — add `E-ART01` / `E-ART02` / `E-ART03` to error code table

---

## 10. Pre-GAN Audit

| # | Weakness | Closed |
|---|---------|--------|
| W1 | `pptxgenjs` failure is silent | `E-ART02` emitted with install hint |
| W2 | Path traversal via `${arg}` injection | `path.resolve()` is safe; no shell expansion |
| W3 | Large sections arrays bloat PPTX | capped at 50 slides (warn if truncated) |
| W4 | Missing parent dir → ENOENT | `mkdir -p` before write |
| W5 | CSV injection via formula prefix (`=`, `+`, `-`, `@`) | values starting with these chars are prefixed with `'` |
