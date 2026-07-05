Status: ready-for-agent
Type: task

# 07: Error screen rendering

## Goal

Implement the error screen in `src/ui.rs` for cases where tuit cannot operate (not a git repo, empty repo, corrupt .git, permission error).

## Layout

```
┌─────────────────────────────────────────────────────┐
│                                                     │
│                    ⚠  Error                         │
│                                                     │
│    tuit は git リポジトリの中で実行してください。    │
│                                                     │
│    [Enter] exit                                      │
│                                                     │
└─────────────────────────────────────────────────────┘
```

## Error cases

| Case | Message |
|------|---------|
| Not a git repo | "tuit は git リポジトリの中で実行してください。" |
| Empty repo (0 commits) | "このリポジトリにはまだコミットがありません。" |
| Corrupt .git | "git リポジトリが壊れています: {detail}" |
| Permission error | "git リポジトリを読み込めません: {detail}" |
| Detached HEAD | No error — show list with "(detached HEAD)" in header |

## Implementation

- A centered block with a warning/error icon, message, and exit instruction
- Use `Paragraph` with alignment and padding
- Press Enter → quit the app
- The screen is rendered when `App.screen == Screen::Error(message)`

## Acceptance

- Error message is displayed centered on screen
- [Enter] exit instruction is visible and works
- Messages for each error case are clear and actionable
- Detached HEAD does NOT show an error — just updates the header
