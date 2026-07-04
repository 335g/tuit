---
Status: Accepted
Date: 2026-07-04
---

# Use gix (gitoxide) instead of shelling out to git CLI

ratty reads commit history and diffs from a git repository. We chose the pure-Rust `gix` library over calling `git log` / `git show` via `std::process::Command`, even though the CLI approach is simpler and more familiar.

## Why

- **Binary portability.** Shelling out requires `git` to be installed and on `$PATH`. gix links statically — the compiled binary works anywhere without system dependencies.
- **Type safety.** gix returns typed objects (commits, trees, oids) rather than unstructured text that needs regex parsing. Parse errors become compile-time issues instead of silent mismatches against a `--format` string.
- **Performance.** gix reads the `.git` object store directly, skipping the fork + exec overhead of spawning a subprocess per operation.

## Trade-offs

- gix has a steeper learning curve than piping CLI output through `serde_json` or custom format strings.
- The gix API is still evolving (pre-1.0); minor version bumps may require adaptation.
- For the simple read-only access pattern ratty needs (walk commit graph, extract diffs) these risks are low.

## Considered alternatives

1. **`git2` (libgit2 bindings).** Feature-rich and stable, but links to a C library, complicating cross-compilation and CI.
2. **`std::process::Command` + git CLI.** Zero added dependency, but couples ratty to the host's git installation and format strings.
