Status: ready-for-agent
Type: task

# 03: Git data layer (gix integration)

## Goal

Implement `src/git.rs` — the module that uses `gix` to open the current repository, walk the current branch's commit history, and retrieve commit details + diffs.

## API surface

```rust
pub struct Commit {
    pub hash: String,        // 短縮7桁
    pub message: String,     // 件名行
    pub author: String,      // 名前のみ
    pub date: String,        // "3d ago" 等の相対時刻文字列
    pub body: String,        // コミット本文（詳細表示時）
    pub diff: String,        // diff テキスト（詳細表示時）
}

pub fn load_commits() -> Result<Vec<Commit>>
pub fn load_diff(hash: &str) -> Result<String>
```

## Details

### `load_commits()`
1. Open the current directory as a gix repository.
2. Resolve HEAD to the current branch.
3. Walk commits reachable from the branch tip (newest first).
4. For each commit, extract: abbreviated hash (7 chars), subject line, author name, relative timestamp.
5. Return `Vec<Commit>` sorted newest-first.

*Note*: For now, `body` and `diff` fields can be empty strings in the list — they're populated lazily by `load_diff` when the user selects a commit.

### `load_diff(hash)`
1. Resolve the full OID from the abbreviated hash.
2. Compute the diff between the commit's tree and its parent tree.
3. Return the diff as a unified-diff formatted string.

*Note on merge commits*: For merge commits with multiple parents, diff against the first parent only.

### Relative time formatting
A helper function to convert a timestamp to a human-readable relative string:
- `< 1 min` → `"just now"`
- `< 60 min` → `"{n}m ago"`
- `< 24 hrs` → `"{n}h ago"`
- `< 30 days` → `"{n}d ago"`
- `< 12 months` → `"{n}mo ago"`
- otherwise → `"{n}y ago"`

## Acceptance

- `load_commits()` returns commits from the current repo (ratteddy itself)
- Format matches the PRD spec (7-char hash, author name, relative date)
- `load_diff()` returns a unified-diff string for a given commit hash
- Handles edge cases: empty repo (no commits), detached HEAD, non-git directory
- Each error case returns a descriptive `anyhow::Error` (not a panic)

## Testing note

This module is exercised indirectly through the single end-to-end seam: tests run tuit inside temporary git repositories and assert on the rendered Commit List and Commit Detail.
