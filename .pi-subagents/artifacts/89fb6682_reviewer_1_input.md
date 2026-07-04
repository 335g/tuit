# Task for reviewer

[Read from: /Users/335g/dev/rust/ratty/AGENTS.md, /Users/335g/dev/rust/ratty/docs/agents/issue-tracker.md, /Users/335g/dev/rust/ratty/docs/agents/triage-labels.md, /Users/335g/dev/rust/ratty/docs/agents/domain.md]

Review the project configuration setup:

1. AGENTS.md — check the Agent skills section references the right docs/agents/* files
2. docs/agents/issue-tracker.md — verify the local-markdown conventions are clear
3. docs/agents/triage-labels.md — verify the label mapping is correct
4. docs/agents/domain.md — verify the single-context layout description

Report any inconsistencies, missing information, or improvements.

---
Update progress at: /Users/335g/dev/rust/ratty/.pi-subagents/artifacts/progress/89fb6682/progress.md

## Acceptance Contract
Acceptance level: checked
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Implement the requested change without widening scope

Required evidence: changed-files, tests-added, commands-run, residual-risks, no-staged-files

Finish with a fenced JSON block tagged `acceptance-report` in this shape:
Use empty arrays when no items apply; array fields contain strings unless object entries are shown.
```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "specific proof"
    }
  ],
  "changedFiles": [
    "src/file.ts"
  ],
  "testsAddedOrUpdated": [
    "test/file.test.ts"
  ],
  "commandsRun": [
    {
      "command": "command",
      "result": "passed",
      "summary": "short result"
    }
  ],
  "validationOutput": [
    "validation output or concise summary"
  ],
  "residualRisks": [
    "none"
  ],
  "noStagedFiles": true,
  "diffSummary": "short description of the diff",
  "reviewFindings": [
    "blocker: file.ts:12 - issue found, or no blockers"
  ],
  "manualNotes": "anything else the parent should know"
}
```