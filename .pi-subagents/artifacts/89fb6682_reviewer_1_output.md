## Review

### 1. AGENTS.md — Agent skills section references
- **Correct**: All three `docs/agents/*` references are valid. `issue-tracker.md`, `triage-labels.md`, and `domain.md` all exist at the referenced paths.
- **No issues found**.

### 2. docs/agents/issue-tracker.md — local-markdown conventions
- **Correct**: Directory structure (`.scratch/<feature-slug>/`), PRD location, issue numbering (01-), and `Status:` line convention are clearly documented.
- **Wayfinding operations** (used by `/wayfinder`) are documented as a separate sub‑system with its own file layout — this is deliberate and scoped correctly.
- **Note (minor)**: The main "Conventions" section says `Status:` should use the triage role strings from `triage-labels.md` (e.g. `ready-for-agent`). The "Wayfinding operations" section uses `Status: claimed` / `Status: resolved`. These are two different vocabularies for two different systems, but the document never explicitly says they are separate. A first-time reader might be confused.
- **No blockers**.

### 3. docs/agents/triage-labels.md — label mapping
- **Correct**: All five canonical labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`) are mapped. The "our tracker" column uses identical strings (identity mapping) — this is consistent and leaves room for future customisation.
- **No issues found**.

### 4. docs/agents/domain.md — single-context layout
- **Correct**: Describes the single-context layout (`CONTEXT.md` at root, `docs/adr/`, `src/`). Matches the actual repo: `CONTEXT.md` exists, `docs/adr/0001-use-gix-over-git-cli.md` exists, `CONTEXT-MAP.md` does not exist (confirming single-context).
- **No issues found**.

### Cross‑document consistency
- AGENTS.md → issue-tracker.md ✓
- AGENTS.md → triage-labels.md ✓
- AGENTS.md → domain.md ✓
- issue-tracker.md → triage-labels.md (referenced for Status values) ✓
- Repo structure matches domain.md's single-context description ✓

### Overall verdict
The configuration setup is consistent and the documentation is accurate. One minor clarification opportunity exists in `issue-tracker.md` (disambiguating the two `Status:` vocabularies), but this is not a blocker.