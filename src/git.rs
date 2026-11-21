use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gix::diff::Options;
use gix::hash::ObjectId;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

/// A single commit's summary data.
#[derive(Clone, Debug)]
pub struct Commit {
    /// 7-character abbreviated hash.
    pub hash: String,
    /// Full OID hex string (40 characters).
    pub oid: String,
    /// Subject line (first line of the commit message).
    pub message: String,
    /// Author name.
    pub author: String,
    /// Relative timestamp (e.g. "3d ago").
    pub date: String,
    /// Full commit body (populated lazily by load_diff).
    pub body: String,
    /// Unified diff text (populated lazily by load_diff).
    pub diff: String,
}

/// Open the given directory as a git repository, walking up the file system
/// from `path` to discover the `.git` directory.
pub fn open_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path).with_context(|| {
        format!(
            "tuit は git リポジトリの中で実行してください: {}",
            path.display()
        )
    })
}

/// Return the current branch name (e.g. `"main"`), or `"HEAD"` if detached.
pub fn current_branch(repo: &gix::Repository) -> Result<String> {
    let head = repo.head().context("No HEAD reference found")?;
    Ok(match head.referent_name() {
        Some(name) => name.shorten().to_string(),
        None => "HEAD".to_string(),
    })
}

/// Return the hex string of the HEAD commit OID from an already-opened repository.
pub fn current_head_oid(repo: &gix::Repository) -> Result<String> {
    let mut head = repo.head().context("No HEAD reference found")?;
    let commit = head
        .peel_to_commit_in_place()
        .map_err(|_| anyhow::anyhow!("このリポジトリにはまだコミットがありません。"))?;
    Ok(commit.id().to_hex().to_string())
}

/// Check whether a git object identified by its hex OID still exists in the repository.
pub fn object_exists(repo: &gix::Repository, oid_hex: &str) -> Result<bool> {
    let oid: gix::hash::ObjectId = oid_hex.parse().context("Invalid OID format")?;
    match repo.find_object(oid) {
        Ok(_) => Ok(true),
        Err(gix::object::find::existing::Error::NotFound { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Load all commits reachable from HEAD (newest first), opening a fresh repository.
pub fn load_commits(path: &Path) -> Result<Vec<Commit>> {
    let repo = open_repo(path)?;
    load_commits_from(&repo)
}

/// Load all commits reachable from HEAD using an already-opened repository.
pub fn load_commits_from(repo: &gix::Repository) -> Result<Vec<Commit>> {
    // Resolve HEAD to a commit OID.
    let mut head = repo.head().context("No HEAD reference found")?;
    let head_id = match head.peel_to_commit_in_place() {
        Ok(commit) => commit.id(),
        Err(_) => anyhow::bail!("このリポジトリにはまだコミットがありません。"),
    };

    // Walk commits newest-first.
    let walk = repo
        .rev_walk(Some(head_id))
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .context("Failed to create revision walk")?;

    let mut commits = Vec::new();
    for entry in walk {
        let info = entry.context("Error walking commits")?;
        let commit_obj = info.object().context("Failed to read commit object")?;

        let oid_hex = info.id().to_hex().to_string();
        let short_hash = oid_hex.chars().take(7).collect::<String>();

        let msg_ref = commit_obj.message().context("Failed to decode message")?;
        let subject = msg_ref.title.to_string().trim_end().to_string();

        let author = commit_obj.author().context("Failed to decode author")?;
        let author_name = author.name.to_string();

        let time_seconds = commit_obj
            .committer()
            .context("Failed to decode committer")?
            .seconds();
        let date_str = format_relative_time(time_seconds);

        commits.push(Commit {
            hash: short_hash,
            oid: oid_hex,
            message: subject,
            author: author_name,
            date: date_str,
            body: String::new(),
            diff: String::new(),
        });
    }

    Ok(commits)
}

/// Load the full commit body and diff for a commit identified by its full OID hex string.
///
/// Returns `(body, diff_text)`.
pub fn load_diff(path: &Path, oid_hex: &str) -> Result<(String, String)> {
    let repo = open_repo(path)?;

    let oid: gix::hash::ObjectId = oid_hex.parse().context("Invalid OID format")?;

    let commit_obj = repo.find_commit(oid).context("Commit not found")?;

    let msg_ref = commit_obj.message().context("Failed to decode message")?;
    let body = msg_ref.body.map(|b| b.to_string()).unwrap_or_default();

    // Get the commit tree.
    let tree = repo
        .find_tree(commit_obj.tree_id()?)
        .context("Failed to find commit tree")?;

    // Get parent tree (first parent only for merge commits).
    let parent_tree = commit_obj.parent_ids().next().and_then(|pid| {
        repo.find_commit(pid.detach())
            .ok()
            .and_then(|pc| pc.tree_id().ok())
            .and_then(|tid| repo.find_tree(tid.detach()).ok())
    });

    // Build unified diff.
    let diff_text = build_diff(&repo, parent_tree.as_ref(), &tree)?;

    Ok((body, diff_text))
}

/// Build a unified-diff formatted string between an optional old tree and a new tree.
fn build_diff(
    repo: &gix::Repository,
    old_tree: Option<&gix::Tree<'_>>,
    new_tree: &gix::Tree<'_>,
) -> Result<String> {
    use gix::object::tree::diff::ChangeDetached;

    // Build diff options with path tracking and no rename tracking.
    let mut opts = Options::default();
    opts.track_path();
    opts.track_rewrites(None);

    let changes: Vec<ChangeDetached> = repo
        .diff_tree_to_tree(old_tree, Some(new_tree), opts)
        .context("Failed to diff trees")?;

    let mut out = String::new();

    for change in &changes {
        match change {
            ChangeDetached::Addition {
                location,
                entry_mode: _,
                relation: _,
                id,
            } => {
                let path = location.to_string();
                out.push_str(&format!("diff --git a/dev/null b/{path}\n"));
                out.push_str("--- /dev/null\n");
                out.push_str(&format!("+++ b/{path}\n"));

                if let Ok(blob) = repo.find_object(*id) {
                    let content = String::from_utf8_lossy(&blob.data);
                    let lines: Vec<&str> = content.lines().collect();
                    let n = if content.is_empty() {
                        0
                    } else {
                        lines.len() + content.ends_with('\n') as usize
                    };
                    if n > 0 {
                        out.push_str(&format!("@@ -0,0 +1,{n} @@\n"));
                        for line in lines {
                            out.push('+');
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
            }
            ChangeDetached::Deletion {
                location,
                entry_mode: _,
                relation: _,
                id,
            } => {
                let path = location.to_string();
                out.push_str(&format!("diff --git a/{path} b/dev/null\n"));
                out.push_str(&format!("--- a/{path}\n"));
                out.push_str("+++ /dev/null\n");

                if let Ok(blob) = repo.find_object(*id) {
                    let content = String::from_utf8_lossy(&blob.data);
                    let lines: Vec<&str> = content.lines().collect();
                    let n = if content.is_empty() {
                        0
                    } else {
                        lines.len() + content.ends_with('\n') as usize
                    };
                    if n > 0 {
                        out.push_str(&format!("@@ -1,{n} +0,0 @@\n"));
                        for line in lines {
                            out.push('-');
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
            }
            ChangeDetached::Modification {
                location,
                previous_entry_mode: _,
                previous_id,
                entry_mode: _,
                id,
            } => {
                let path = location.to_string();
                out.push_str(&format!("diff --git a/{path} b/{path}\n"));
                out.push_str(&format!("--- a/{path}\n"));
                out.push_str(&format!("+++ b/{path}\n"));

                let old_content = repo.find_object(*previous_id).ok().map(|o| o.data.to_vec());
                let new_content = repo.find_object(*id).ok().map(|o| o.data.to_vec());

                match (old_content, new_content) {
                    (Some(old), Some(new)) => {
                        let old_str = String::from_utf8_lossy(&old).to_string();
                        let new_str = String::from_utf8_lossy(&new).to_string();
                        append_unified_diff(&mut out, &old_str, &new_str);
                    }
                    (Some(old), None) => {
                        let content = String::from_utf8_lossy(&old);
                        let lines: Vec<&str> = content.lines().collect();
                        let n = if content.is_empty() {
                            0
                        } else {
                            lines.len() + content.ends_with('\n') as usize
                        };
                        if n > 0 {
                            out.push_str(&format!("@@ -1,{n} +0,0 @@\n"));
                            for line in lines {
                                out.push('-');
                                out.push_str(line);
                                out.push('\n');
                            }
                        }
                    }
                    (None, Some(new)) => {
                        let content = String::from_utf8_lossy(&new);
                        let lines: Vec<&str> = content.lines().collect();
                        let n = if content.is_empty() {
                            0
                        } else {
                            lines.len() + content.ends_with('\n') as usize
                        };
                        if n > 0 {
                            out.push_str(&format!("@@ -0,0 +1,{n} @@\n"));
                            for line in lines {
                                out.push('+');
                                out.push_str(line);
                                out.push('\n');
                            }
                        }
                    }
                    (None, None) => {}
                }
            }
            ChangeDetached::Rewrite { .. } => {
                // Phase 1: skip rewrite entries (rename/copy is disabled anyway).
            }
        }
    }

    Ok(out)
}

/// Append a unified-diff hunk for the two text contents using imara-diff.
fn append_unified_diff(out: &mut String, old: &str, new: &str) {
    use imara_diff::UnifiedDiffBuilder;
    use imara_diff::intern::InternedInput;
    use imara_diff::{Algorithm, diff};

    if old.is_empty() && new.is_empty() {
        return;
    }

    let input = InternedInput::new(old, new);
    let builder = UnifiedDiffBuilder::new(&input);
    let result = diff(Algorithm::Histogram, &input, builder);
    if !result.is_empty() {
        out.push_str(&result);
    }
}

/// Convert a Unix timestamp to a human-readable relative time string.
fn format_relative_time(seconds: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let diff = now - seconds;
    if diff < 0 {
        return "just now".into();
    }

    let minutes = diff / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let months = days / 30;
    let years = months / 12;

    if minutes < 1 {
        "just now".into()
    } else if minutes < 60 {
        format!("{}m ago", minutes)
    } else if hours < 24 {
        format!("{}h ago", hours)
    } else if days < 30 {
        format!("{}d ago", days)
    } else if months < 12 {
        format!("{}mo ago", months)
    } else {
        format!("{}y ago", years)
    }
}

// ── Worktree dashboard ─────────────────────────────────────────────

/// Maximum number of commits included in the topology graph.
const MAX_GRAPH_COMMITS: usize = 120;
/// Days covered by the activity sparkline.
const ACTIVITY_DAYS: usize = 14;

/// Summary of a single worktree (the main worktree or a linked one).
#[derive(Clone, Debug)]
pub struct Worktree {
    /// Worktree id: `"main"` or the linked-worktree folder name.
    pub name: String,
    /// Filesystem path of the checkout.
    pub path: PathBuf,
    /// Checked-out branch short name, or `"detached"`.
    pub branch: String,
    /// Full OID of HEAD (empty when the worktree has no commits).
    pub head_oid: String,
    /// 7-character abbreviated HEAD hash (empty when there are no commits).
    pub head_hash: String,
    /// HEAD commit subject (empty when there are no commits).
    pub head_subject: String,
    /// Commits reachable from this HEAD but not from the base.
    pub ahead: usize,
    /// Commits reachable from the base but not from this HEAD.
    pub behind: usize,
    /// Whether this is the main worktree.
    pub is_main: bool,
    /// True if the worktree has uncommitted changes.
    /// shortcut: untracked files are not reported by `Repository::is_dirty()`.
    pub dirty: bool,
    /// Commit counts per day, oldest → newest (len == ACTIVITY_DAYS).
    pub activity: Vec<u64>,
}

/// One rendered line of the topology graph.
#[derive(Clone, Debug)]
pub struct GraphLine {
    /// Full line: connectors + node + hash + refs + subject.
    pub text: String,
    /// Branch names whose HEAD sits on this line's commit.
    pub refs: Vec<String>,
}

/// Topology graph spanning all worktree branches.
#[derive(Clone, Debug)]
pub struct WorktreeGraph {
    pub lines: Vec<GraphLine>,
}

/// Load all worktrees (main + linked) and the topology graph across them.
///
/// The base for ahead/behind is the main worktree's HEAD, falling back to
/// the HEAD of the repository tuit was opened at (bare-parent case).
///
/// shortcut: ahead/behind walks each branch once per call; fine up to a few
/// thousand commits, revisit with commit-graph caching if polling gets slow.
pub fn load_worktrees(path: &Path) -> Result<(Vec<Worktree>, WorktreeGraph)> {
    let repo = open_repo(path)?;
    let main_repo = match repo.main_repo() {
        Ok(r) => r,
        Err(_) => repo.clone(),
    };

    struct Desc {
        name: String,
        path: PathBuf,
        is_main: bool,
        repo: Option<gix::Repository>,
    }

    let mut descs: Vec<Desc> = Vec::new();
    if let Some(wd) = main_repo.workdir() {
        descs.push(Desc {
            name: "main".to_string(),
            path: wd.to_path_buf(),
            is_main: true,
            repo: Some(main_repo.clone()),
        });
    }
    for proxy in main_repo.worktrees()? {
        descs.push(Desc {
            name: proxy.id().to_string(),
            path: proxy.base().unwrap_or_default(),
            is_main: false,
            repo: proxy
                .into_repo_with_possibly_inaccessible_worktree()
                .ok(),
        });
    }

    let mut branches: Vec<String> = Vec::with_capacity(descs.len());
    let mut heads: Vec<Option<ObjectId>> = Vec::with_capacity(descs.len());
    for d in &descs {
        let (branch, head) = match &d.repo {
            Some(r) => (worktree_branch(r), head_commit_id(r)),
            None => ("?".to_string(), None),
        };
        branches.push(branch);
        heads.push(head);
    }

    let base_oid = descs
        .iter()
        .position(|d| d.is_main)
        .and_then(|i| heads[i])
        .or_else(|| head_commit_id(&repo));

    let mut worktrees = Vec::with_capacity(descs.len());
    for (i, d) in descs.iter().enumerate() {
        let (ahead, behind, activity, head_hash, head_subject) = match (heads[i], base_oid) {
            (Some(head), Some(base)) => {
                let (a, b) = ahead_behind(&repo, base, head).unwrap_or((0, 0));
                let act = recent_activity(&repo, head, ACTIVITY_DAYS).unwrap_or_default();
                let (hash, subject) = head_label(&repo, head);
                (a, b, act, hash, subject)
            }
            (Some(head), None) => {
                let act = recent_activity(&repo, head, ACTIVITY_DAYS).unwrap_or_default();
                let (hash, subject) = head_label(&repo, head);
                (0, 0, act, hash, subject)
            }
            (None, _) => (0, 0, vec![0; ACTIVITY_DAYS], String::new(), String::new()),
        };

        let dirty = d
            .repo
            .as_ref()
            .map(|r| r.is_dirty().unwrap_or(false))
            .unwrap_or(false);

        worktrees.push(Worktree {
            name: d.name.clone(),
            path: d.path.clone(),
            branch: branches[i].clone(),
            head_oid: heads[i]
                .map(|o| o.to_hex().to_string())
                .unwrap_or_default(),
            head_hash,
            head_subject,
            ahead,
            behind,
            is_main: d.is_main,
            dirty,
            activity,
        });
    }

    let graph = build_graph(&repo, &worktrees)?;
    Ok((worktrees, graph))
}

/// Checked-out branch short name, or `"detached"`.
fn worktree_branch(repo: &gix::Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.referent_name().map(|n| n.shorten().to_string()))
        .unwrap_or_else(|| "detached".to_string())
}

/// HEAD commit OID, or `None` when the worktree has no commits.
fn head_commit_id(repo: &gix::Repository) -> Option<ObjectId> {
    let mut head = repo.head().ok()?;
    head.peel_to_commit_in_place().ok().map(|c| c.id().detach())
}

/// (7-char hash, subject) of a commit, empty on failure.
fn head_label(repo: &gix::Repository, oid: ObjectId) -> (String, String) {
    match repo.find_commit(oid) {
        Ok(c) => {
            let hash = oid.to_hex().to_string().chars().take(7).collect::<String>();
            let subject = c
                .message()
                .map(|m| m.title.to_string().trim_end().to_string())
                .unwrap_or_default();
            (hash, subject)
        }
        Err(_) => (String::new(), String::new()),
    }
}

/// (ahead, behind) of `head` relative to `base`, via the merge base.
fn ahead_behind(
    repo: &gix::Repository,
    base: ObjectId,
    head: ObjectId,
) -> Result<(usize, usize)> {
    match repo.merge_base(base, head) {
        Ok(mb) => {
            let mb = mb.detach();
            let ahead = count_reachable_excluding(repo, head, mb)?;
            let behind = count_reachable_excluding(repo, base, mb)?;
            Ok((ahead, behind))
        }
        // Unrelated histories: everything counts as ahead/behind.
        Err(_) => {
            let ahead = count_reachable(repo, head)?;
            let behind = count_reachable(repo, base)?;
            Ok((ahead, behind))
        }
    }
}

fn count_reachable(repo: &gix::Repository, head: ObjectId) -> Result<usize> {
    let walk = repo
        .rev_walk(Some(head))
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .context("Failed to create revision walk")?;
    let mut n = 0;
    for entry in walk {
        entry.context("Error walking commits")?;
        n += 1;
    }
    Ok(n)
}

/// Commits reachable from `head` without crossing `exclude` (the merge base).
///
/// shortcut: walks the whole ahead-chain once per call; fine up to a few
/// thousand commits, revisit with commit-graph caching if polling gets slow.
fn count_reachable_excluding(
    repo: &gix::Repository,
    head: ObjectId,
    exclude: ObjectId,
) -> Result<usize> {
    let walk = repo
        .rev_walk(Some(head))
        .with_boundary(std::iter::once(exclude))
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .context("Failed to create revision walk")?;
    let mut n = 0;
    for entry in walk {
        entry.context("Error walking commits")?;
        n += 1;
    }
    Ok(n)
}

/// Commit counts per day over the last `days` days, oldest → newest.
fn recent_activity(
    repo: &gix::Repository,
    head: ObjectId,
    days: usize,
) -> Result<Vec<u64>> {
    let walk = repo
        .rev_walk(Some(head))
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .context("Failed to create revision walk")?;

    let now = now_seconds();
    let cutoff = now - (days as i64) * 86400;
    let mut hist = vec![0u64; days];

    for entry in walk {
        let info = entry.context("Error walking commits")?;
        let commit_obj = info.object().context("Failed to read commit object")?;
        let ts = commit_obj.committer().map(|s| s.seconds()).unwrap_or(0);
        if ts < cutoff {
            break; // newest-first: everything after this is outside the window
        }
        let age_days = (now - ts).max(0) / 86400;
        if (age_days as usize) < days {
            hist[days - 1 - age_days as usize] += 1;
        }
    }
    Ok(hist)
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// A commit node in the topology graph.
#[derive(Clone, Debug)]
struct GraphNode {
    parents: Vec<ObjectId>,
    time: i64,
    subject: String,
    refs: Vec<String>,
}

/// Build a `git log --graph`-style topology graph across all worktree heads.
fn build_graph(repo: &gix::Repository, worktrees: &[Worktree]) -> Result<WorktreeGraph> {
    let heads: Vec<ObjectId> = worktrees
        .iter()
        .filter_map(|w| w.head_oid.parse::<ObjectId>().ok())
        .collect();
    if heads.is_empty() {
        return Ok(WorktreeGraph { lines: Vec::new() });
    }

    let walk = repo
        .rev_walk(heads.iter().copied())
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .context("Failed to create revision walk")?;

    let mut nodes: HashMap<ObjectId, GraphNode> = HashMap::new();
    let mut order: Vec<ObjectId> = Vec::new();
    for entry in walk {
        let info = entry.context("Error walking commits")?;
        let oid = info.id().detach();
        let commit_obj = info.object().context("Failed to read commit object")?;
        let parents = commit_obj
            .parent_ids()
            .map(|p| p.detach())
            .collect::<Vec<_>>();
        let time = commit_obj.committer().map(|s| s.seconds()).unwrap_or(0);
        let subject = commit_obj
            .message()
            .map(|m| m.title.to_string().trim_end().to_string())
            .unwrap_or_default();
        nodes.insert(
            oid,
            GraphNode {
                parents,
                time,
                subject,
                refs: Vec::new(),
            },
        );
        order.push(oid);
        if order.len() >= MAX_GRAPH_COMMITS {
            break;
        }
    }

    for w in worktrees {
        if let Ok(oid) = w.head_oid.parse::<ObjectId>() {
            if let Some(node) = nodes.get_mut(&oid) {
                node.refs.push(w.branch.clone());
            }
        }
    }

    // Children adjacency: newer commits that point at this commit.
    let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (&oid, node) in &nodes {
        for p in &node.parents {
            if nodes.contains_key(p) {
                children.entry(*p).or_default().push(oid);
            }
        }
    }

    let topo = topo_order(&nodes, &children);

    // Column assignment (git-like, simplified).
    let mut col: HashMap<ObjectId, usize> = HashMap::new();
    let mut reserved: HashMap<ObjectId, usize> = HashMap::new();
    let mut next_col = 0usize;
    for oid in &topo {
        let node = &nodes[oid];
        let child_cols: Vec<usize> = children
            .get(oid)
            .map(|cs| {
                let mut v: Vec<usize> = cs.iter().filter_map(|c| col.get(c).copied()).collect();
                v.sort_unstable();
                v.dedup();
                v
            })
            .unwrap_or_default();

        let c = if let Some(&r) = reserved.get(oid) {
            r
        } else if !child_cols.is_empty() {
            child_cols[0]
        } else {
            let c = next_col;
            next_col += 1;
            c
        };
        col.insert(*oid, c);
        next_col = next_col.max(c + 1);

        for (i, p) in node.parents.iter().enumerate() {
            if !nodes.contains_key(p) {
                continue;
            }
            if i == 0 {
                reserved
                    .entry(*p)
                    .and_modify(|v| *v = (*v).min(c))
                    .or_insert(c);
            } else {
                reserved.entry(*p).or_insert_with(|| {
                    let c = next_col;
                    next_col += 1;
                    c
                });
            }
        }
    }

    let row: HashMap<ObjectId, usize> = topo
        .iter()
        .enumerate()
        .map(|(i, &oid)| (oid, i))
        .collect();
    let max_col = col.values().copied().max().unwrap_or(0);
    let n_rows = topo.len();

    let mut grid: Vec<Vec<char>> = vec![vec![' '; max_col + 1]; n_rows];
    for oid in &topo {
        grid[row[oid]][col[oid]] = '●';
    }

    // Connectors from each commit down to its parents.
    for oid in &topo {
        let rc = row[oid];
        let cc = col[oid];
        for p in nodes[oid]
            .parents
            .iter()
            .filter(|p| nodes.contains_key(*p))
        {
            let rp = row[p];
            let cp = col[p];
            if cp == cc {
                for r in (rc + 1)..rp {
                    grid[r][cc] = '│';
                }
            } else if cp > cc {
                if rc + 1 < n_rows {
                    grid[rc + 1][cp] = '╲';
                }
                for r in (rc + 2)..rp {
                    grid[r][cp] = '│';
                }
            } else {
                for r in (rc + 1)..rp {
                    grid[r][cc] = '│';
                }
                if rp > rc + 1 {
                    grid[rp - 1][cc] = '╱';
                }
            }
        }
    }

    let mut lines = Vec::with_capacity(n_rows);
    for i in 0..n_rows {
        let oid = topo[i];
        let node = &nodes[&oid];
        let last_connector = grid[i]
            .iter()
            .rposition(|&ch| ch != ' ')
            .unwrap_or(col[&oid]);
        let mut s = String::new();
        for j in 0..=last_connector {
            s.push(grid[i][j]);
        }
        let hash = oid.to_hex().to_string().chars().take(7).collect::<String>();
        let refs_str = if node.refs.is_empty() {
            String::new()
        } else {
            format!(" ({})", node.refs.join(", "))
        };
        s.push(' ');
        s.push_str(&hash);
        s.push_str(&refs_str);
        s.push(' ');
        s.push_str(&node.subject);
        lines.push(GraphLine {
            text: s,
            refs: node.refs.clone(),
        });
    }

    Ok(WorktreeGraph { lines })
}

/// Topological order: children (newer) before parents (older), ties by commit
/// time descending. Heads have depth 0; each parent sits one level below its
/// deepest child, so every commit precedes its parents.
fn topo_order(
    nodes: &HashMap<ObjectId, GraphNode>,
    children: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Vec<ObjectId> {
    fn depth(
        oid: ObjectId,
        children: &HashMap<ObjectId, Vec<ObjectId>>,
        memo: &mut HashMap<ObjectId, usize>,
    ) -> usize {
        if let Some(&d) = memo.get(&oid) {
            return d;
        }
        let d = children
            .get(&oid)
            .map(|cs| {
                cs.iter()
                    .map(|c| depth(*c, children, memo))
                    .max()
                    .unwrap_or(0)
                    + 1
            })
            .unwrap_or(0);
        memo.insert(oid, d);
        d
    }

    let mut memo = HashMap::new();
    let mut order: Vec<ObjectId> = nodes.keys().copied().collect();
    order.sort_by(|a, b| {
        let da = depth(*a, children, &mut memo);
        let db = depth(*b, children, &mut memo);
        da.cmp(&db)
            .then_with(|| nodes[b].time.cmp(&nodes[a].time))
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_time() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert_eq!(format_relative_time(now), "just now");
        assert_eq!(format_relative_time(now - 30), "just now");
        assert_eq!(format_relative_time(now - 120), "2m ago");
        assert_eq!(format_relative_time(now - 3600), "1h ago");
        assert_eq!(format_relative_time(now - 7200), "2h ago");
        assert_eq!(format_relative_time(now - 86400), "1d ago");
        assert_eq!(format_relative_time(now - 86400 * 5), "5d ago");
        assert_eq!(format_relative_time(now - 86400 * 60), "2mo ago");
        assert_eq!(format_relative_time(now - 86400 * 400), "1y ago");
    }

    #[test]
    fn worktrees_ahead_behind_and_graph() {
        use std::process::Command;

        let base = std::env::temp_dir().join("tuit-worktrees-e2e");
        let main = base.join("main");
        let wta = base.join("feat-a");
        let wtb = base.join("feat-b");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&main).unwrap();

        let commit = |dir: &Path, msg: &str| {
            let file = dir.join(format!("f-{}.txt", msg.replace(' ', "-")));
            std::fs::write(&file, msg).unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "add", "."])
                .status()
                .unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "commit", "-m", msg])
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "t@e.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "t@e.com")
                .status()
                .unwrap();
        };

        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&main)
            .status()
            .unwrap();
        commit(&main, "main 0");
        commit(&main, "main 1");
        commit(&main, "main 2");

        // feat-a: 2 commits ahead of main.
        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "worktree", "add", "-b", "feat-a"])
            .arg(&wta)
            .status()
            .unwrap();
        commit(&wta, "feat a 1");
        commit(&wta, "feat a 2");

        // feat-b: 1 commit ahead, forked from main.
        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "branch", "feat-b"])
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "worktree", "add"])
            .arg(&wtb)
            .arg("feat-b")
            .status()
            .unwrap();
        commit(&wtb, "feat b 1");

        let (worktrees, graph) = load_worktrees(&main).unwrap();

        let main_wt = worktrees
            .iter()
            .find(|w| w.branch == "main")
            .expect("main worktree");
        let a = worktrees.iter().find(|w| w.branch == "feat-a").unwrap();
        let b = worktrees.iter().find(|w| w.branch == "feat-b").unwrap();

        assert!(main_wt.is_main);
        assert_eq!(main_wt.ahead, 0);
        assert_eq!(main_wt.behind, 0);
        assert_eq!(a.ahead, 2);
        assert_eq!(a.behind, 0);
        assert_eq!(b.ahead, 1);
        assert_eq!(b.behind, 0);

        let all_text: String = graph
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("feat-a"), "graph:\n{all_text}");
        assert!(all_text.contains("feat-b"), "graph:\n{all_text}");
        assert!(all_text.contains("feat a 2"), "graph:\n{all_text}");
        assert!(all_text.contains("main 0"), "graph:\n{all_text}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn worktrees_from_bare_parent() {
        use std::process::Command;

        let base = std::env::temp_dir().join("tuit-bare-parent");
        let bare = base.join("repo.git");
        let wt1 = base.join("wt1");
        let wt2 = base.join("wt2");
        let _ = std::fs::remove_dir_all(&base);

        // Seed repo with one commit, then clone --bare.
        let seed = base.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&seed)
            .status()
            .unwrap();
        std::fs::write(seed.join("f.txt"), "x").unwrap();
        Command::new("git")
            .args(["-C", &seed.to_string_lossy(), "add", "."])
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &seed.to_string_lossy(), "commit", "-m", "seed"])
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "t@e.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "t@e.com")
            .status()
            .unwrap();
        Command::new("git")
            .args(["clone", "--bare"])
            .arg(&seed)
            .arg(&bare)
            .status()
            .unwrap();

        // Two worktrees from the bare parent.
        Command::new("git")
            .args(["-C", &bare.to_string_lossy(), "worktree", "add", "-b", "wt1"])
            .arg(&wt1)
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &bare.to_string_lossy(), "worktree", "add", "-b", "wt2"])
            .arg(&wt2)
            .status()
            .unwrap();

        let (worktrees, _graph) = load_worktrees(&wt1).unwrap();
        let branches: Vec<&str> = worktrees.iter().map(|w| w.branch.as_str()).collect();
        assert!(
            branches.contains(&"wt1") && branches.contains(&"wt2"),
            "expected wt1 and wt2, got {branches:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
