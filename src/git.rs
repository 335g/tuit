use anyhow::{Context, Result};
use gix::diff::Options;
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

/// Open the current directory as a git repository.
pub fn open_repo() -> Result<gix::Repository> {
    gix::open(".").context("tuit は git リポジトリの中で実行してください。")
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
pub fn load_commits() -> Result<Vec<Commit>> {
    let repo = open_repo()?;
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
        let subject = msg_ref.title.to_string();

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
pub fn load_diff(oid_hex: &str) -> Result<(String, String)> {
    let repo = open_repo()?;

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
}
