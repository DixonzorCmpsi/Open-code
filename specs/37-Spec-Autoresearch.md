# Spec 37: Spec Autoresearch

**Status:** Specced 2026-03-19. Applies the Karpathy autoresearch framework (Spec 35) to spec documents themselves — automating the GAN audit loop that has so far been run manually for every spec. Defines `claw spec-check` (single-pass LLM quality audit), `claw spec-tune` (full mutation loop), and a standard set of binary spec quality criteria.

---

## 1. The Problem with Manual GAN Audits

The GAN audit process used for Specs 32–36 has been effective: every spec has gone through 2+ rounds of Maker/Breaker passes, catching between 8–18 gaps per spec. But the process has three weaknesses:

1. **It requires me as the adversary.** The Breaker pass depends on the same author who wrote the spec trying hard to break it. Authors have blind spots — the same assumptions that went into the spec go into the audit.
2. **It is not exhaustive.** A single Breaker pass might miss gaps that a different adversarial framing would find. The best audit would run the Breaker pass dozens of times with varied framings.
3. **There is no feedback on spec quality over time.** We have no way to measure whether Spec 36 is a better-written spec than Spec 32, or whether the GAN audit process itself is improving.

The autoresearch framework (Spec 35) already solves this for synthesis prompts. The exact same three ingredients exist for specs:

| Autoresearch ingredient | Spec equivalent |
|---|---|
| Objective metric | Spec quality score: sum of binary criteria passes |
| Automated measurement | LLM judge evaluating spec markdown against criteria |
| Something to change | The spec text, grammar definitions, error code tables, examples |

---

## 2. Standard Spec Quality Criteria

These binary yes/no questions apply to every Claw spec. They are the `eval {}` criteria for specs.

### 2.1 Completeness criteria

```
grammar_examples:     "Does every new DSL construct have a .claw syntax example?"
ast_defined:          "Does every new AST node have a Rust struct/enum definition?"
error_codes:          "Are all error/warning codes in a table with name, code, and trigger?"
parser_note:          "Does the spec note any parser constraints (e.g. token ambiguities, precedence)?"
codegen_outputs:      "Does the spec list every file that codegen will produce?"
```

### 2.2 Consistency criteria

```
cross_refs_valid:     "Do all references to other specs (e.g. 'Spec 33 §2') name real sections?"
no_contradiction:     "Does the spec avoid contradicting itself within a single section?"
field_names_stable:   "Are field names used consistently throughout (no 'foo' vs 'foo_bar' drift)?"
defaults_stated:      "Does every optional config field state its default value explicitly?"
```

### 2.3 Implementability criteria

```
ambiguous_behavior:   "Is there at least one example of expected behavior for every error code?"
type_signatures:      "Are all function/method signatures shown (not just described in prose)?"
integration_clear:    "Does each new feature state which existing codegen files need changes?"
no_vague_language:    "Does the spec avoid unresolvable vague phrases like 'reasonable', 'appropriate', 'if needed'?"
```

### 2.4 Edge case criteria

```
empty_inputs:         "Does the spec define behavior when optional blocks are absent?"
conflict_handling:    "Does the spec define behavior when two features interact (e.g. retry + tune)?"
offline_behavior:     "Does the spec define behavior when the LLM/API is unavailable?"
```

---

## 3. `claw spec-check` — Single Pass Audit

```bash
claw spec-check <spec.md> [--criteria <criteria-file>] [--client <client-name>]
```

Runs the standard 16 criteria above (or a custom set) against a spec file using a single LLM judge call. Output:

```
claw spec-check specs/36-Synthesis-Repair-Loop.md

Evaluating 16 criteria against specs/36-Synthesis-Repair-Loop.md...

  grammar_examples     YES
  ast_defined          YES
  error_codes          YES
  parser_note          YES
  codegen_outputs       NO  ← "spec does not list all generated files"
  cross_refs_valid     YES
  no_contradiction     YES
  field_names_stable    NO  ← "compile_repair_limit vs compile_limit used interchangeably"
  defaults_stated      YES
  ambiguous_behavior   YES
  type_signatures      YES
  integration_clear     NO  ← "spec does not name which codegen files change"
  no_vague_language    YES
  empty_inputs         YES
  conflict_handling    YES
  offline_behavior      NO  ← "offline behavior when tsc not installed not addressed"

Score: 12/16 (75%)

4 criteria failed. Run 'claw spec-check --report' for detailed suggestions.
```

The judge prompt follows the same structure as the Spec 35 eval judge (single call, all criteria in order, YES/NO per line). The judge is asked to output a one-line explanation for each NO.

---

## 4. `claw spec-tune` — Mutation Loop

```bash
claw spec-tune <spec.md> [--iterations <n>] [--dry-run] [--client <client-name>]
```

Applies the Spec 35 autoresearch loop to spec markdown instead of synthesis prompts:

```
current_spec ← spec.md content
best_spec    ← current_spec
best_score   ← 0

for iteration in 1..=iterations:
    score ← judge(current_spec, criteria)

    if score > best_score:
        best_score ← score
        best_spec  ← current_spec

    failures ← criteria where judge answered NO
    current_spec ← mutate_spec(current_spec, failures, meta_llm)

    emit SpecTuneIteration { iteration, score, best_score }
    if score == max: break

write best_spec to <spec.md>.tuned.md   // never overwrites original
emit TuneReport { best_score, gap_analysis }
```

The mutator prompt for specs:

```
SYSTEM:
You are improving a technical specification document. Your goal: ensure it passes all quality criteria.
Output ONLY the improved spec. Do not explain. Preserve all technical content — only add missing information.

USER:
Current spec:
---
<current spec content>
---

Failing criteria:
- "Does the spec list every file that codegen will produce?" — answered NO
- "Does the spec define behavior when the LLM/API is unavailable?" — answered NO

Improve the spec to address these gaps. Do not remove or contradict existing content.
```

**Key difference from synthesis tune:** The mutator is told to ONLY ADD information, never remove or contradict. Specs accumulate correctness — they don't get rewritten from scratch.

---

## 5. `claw spec-cross-check` — Multi-Spec Consistency

```bash
claw spec-cross-check specs/
```

Checks cross-spec consistency across all spec files in a directory. This is the hardest problem in spec authoring: two specs that are internally consistent can contradict each other.

Cross-check criteria (run by a single LLM call reading all specs):

```
interface_stable:     "Do all specs that reference SynthesisRequest agree on its field names?"
error_code_unique:    "Is every error code (E-R01, W-T02, etc.) defined in exactly one spec?"
version_consistent:   "Do all specs reference the same versions of tools (e.g. claude-haiku-4-5)?"
no_orphan_refs:       "Does every spec cross-reference point to a real section in another spec?"
```

Output:

```
claw spec-cross-check specs/

Checking 8 specs for cross-consistency...

  interface_stable    FAIL
    Spec 33 §2 defines SynthesisRequest.tool_name
    Spec 36 §5 references SynthesisRequest.tool but uses "tool_name" elsewhere
    → inconsistency: field called "tool" in repair_context but "tool_name" in synthesis

  error_code_unique   PASS
  version_consistent  PASS
  no_orphan_refs      FAIL
    Spec 36 §3.2 references "Spec 33 §3" — section 3 in spec 33 is titled "§3 Synthesizer Config", not "§3 prompt template"
    → closest match: Spec 33 §4 "Synthesis Prompt Template"

Score: 2/4 cross-checks passed.
```

---

## 6. Integration with the Existing GAN Process

`claw spec-check` does NOT replace the human GAN audit. It complements it:

| What | Who | Catches |
|---|---|---|
| `claw spec-check` (automated) | LLM judge | Structural completeness, missing examples, vague language, dangling refs |
| Human Maker/Breaker pass | Author + adversarial LLM | Deep semantic gaps, integration bugs, edge cases specific to this feature |

The recommended flow for new specs:

1. Write the spec draft.
2. Run `claw spec-check` — fix the 16 standard criteria failures before the human GAN.
3. Run the human Maker/Breaker GAN audit — finds the feature-specific gaps `spec-check` cannot.
4. Apply GAN fixes.
5. Run `claw spec-check` again — confirm all 16 criteria now pass.
6. Optionally run `claw spec-tune` for iterative refinement.

This front-loads the mechanical quality check so the human GAN audit focuses on the hard problems.

---

## 7. `spec-criteria` File Format

Custom criteria files use `.spec-criteria` format (YAML):

```yaml
# my-project.spec-criteria
criteria:
  grammar_examples:
    question: "Does every new DSL construct have a .claw syntax example?"
  ast_defined:
    question: "Does every new AST node have a Rust struct/enum definition?"
  # ... add project-specific criteria
  migration_guide:
    question: "Does the spec include a migration guide for breaking changes?"
  test_fixtures:
    question: "Does the spec include at least one test fixture for new AST nodes?"
```

This is the `eval {}` block for specs — the same infrastructure, just pointed at markdown instead of TypeScript.

---

## 8. Why This Makes Specs Better

The research finding from automated program repair applies directly: **iterative feedback with precise, binary criteria produces better outputs than one-shot generation**. For specs, the concrete gains are:

1. **Fewer implementation surprises.** The `integration_clear` criterion forces every spec to name the files that change. Implementors spend less time reading the spec trying to infer impact.
2. **Fewer error code ambiguities.** The `error_codes` criterion forces a complete table. Without it, implementors guess at error names and create inconsistencies.
3. **Faster cross-spec consistency.** `spec-cross-check` catches interface drift between specs before it becomes an implementation bug.
4. **Measurable quality over time.** Every spec gets a score (e.g., 14/16). Tracking this across specs shows whether the authoring process is improving.

The specs written before Spec 37 (32–36) average approximately 11/16 on the standard criteria based on a retrospective assessment. Post-Spec-37 specs should consistently hit 15–16/16 before the human GAN audit begins.

---

## 9. New CLI Commands

| Command | Description |
|---|---|
| `claw spec-check <file>` | Single-pass quality audit against standard criteria |
| `claw spec-check --criteria <file>` | Use custom criteria file |
| `claw spec-tune <file>` | Mutation loop to improve spec to pass all criteria |
| `claw spec-cross-check <dir>` | Multi-spec consistency check |

These commands are standalone — they don't require a `.claw` source file or a compiled artifact. They only need a Claude API key (or local Ollama client) and the spec markdown.

---

## 10. Spec-Check Amendments (self-applied)

### 10.1 Error and warning codes

| Code | Name | Trigger |
|---|---|---|
| W-SC01 | NoApiKey | `claw spec-check` invoked without API key and no local client configured |
| W-SC02 | EmptySpec | Spec file is empty or has < 3 sections — score is undefined |
| W-SC03 | UnknownCriteria | A custom `.spec-criteria` file references an unknown question key |
| E-SC01 | CriteriaFileMissing | `--criteria <file>` path does not exist |
| E-SC02 | CrossCheckDirMissing | `--dir` path for `spec-cross-check` does not exist or has no `.md` files |

### 10.2 CLI type signatures

```typescript
// claw spec-check
interface SpecCheckArgs {
  file:       string;          // path to spec markdown
  criteria?:  string;          // path to .spec-criteria YAML (default: built-in 16)
  client?:    string;          // client name from claw.json (default: first declared client)
  report?:    boolean;         // print per-criterion explanations (default: false)
}

interface SpecCheckResult {
  file:      string;
  score:     number;           // e.g. 12
  max_score: number;           // e.g. 16
  results:   CriterionResult[];
}

interface CriterionResult {
  label:       string;
  passed:      boolean;
  explanation: string | null;  // non-null when failed
}
```

```typescript
// claw spec-tune
interface SpecTuneArgs {
  file:       string;
  iterations: number;          // default: 10
  dry_run?:   boolean;
  client?:    string;
}
// Output: writes <file>.tuned.md — never overwrites original
```

### 10.3 Integration — what existing files change

`claw spec-check` and `claw spec-tune` are new standalone commands. They do NOT modify the Rust compiler pipeline. Integration points:

| File | Change |
|---|---|
| `src/bin/claw.rs` | Add `spec-check`, `spec-tune`, `spec-cross-check` subcommands |
| `specs/.spec-criteria` | New file format — parsed by `spec_criteria.rs` or inline in `claw.rs` |
| `~/.claw/spec-check-history/` | Written per run (similar to tune-history) |

No changes to `src/ast.rs`, `src/parser.rs`, or any codegen module — these commands operate on markdown only.

### 10.4 Empty / malformed spec behavior

- If the spec file is **empty**: emit `W-SC02`, report score `0/16`, exit 0 (warning, not error).
- If the spec file has **< 3 sections** (fewer than 3 `##` headings): emit `W-SC02` with hint "spec may be incomplete".
- If the spec file **does not exist**: exit 1 with `error: file not found: <path>`.
- The judge is NOT called for empty specs — it would produce unreliable results.

### 10.5 Offline behavior

`claw spec-check` checks at startup (same pattern as `claw tune` §11.6):
1. If `--client` names a local Ollama client → runs offline.
2. If no `--client` and no `claw.json` present → checks `ANTHROPIC_API_KEY` env var.
3. If cloud client configured but no API key → exits with:
   ```
   error: ANTHROPIC_API_KEY not set. Set the key or use --client with a local Ollama client.
   ```
4. `--dry-run` on `claw spec-tune` skips the mutation step and only runs the judge pass — requires an API key for the judge call itself, but no synthesis calls.

### 10.6 Defaults stated explicitly

| Config | Default |
|---|---|
| `--client` | First client in `claw.json`; if no `claw.json`, uses `ANTHROPIC_API_KEY` with `claude-haiku-4-5-20251001` |
| `--iterations` (spec-tune) | 10 |
| `--criteria` | Built-in 16 criteria (§2 of this spec) |
| judge budget | No budget cap (spec-check is a single call; spec-tune: $2.00 default) |

---

## 11. What This Spec Does NOT Cover

- Automatically generating spec drafts from feature descriptions (separate future spec)
- Formal verification of spec correctness against the Rust implementation
- Integration with the `claw tune` infrastructure from Spec 35 at the code level (these are separate binaries)
- Version history diffing of specs across git commits
