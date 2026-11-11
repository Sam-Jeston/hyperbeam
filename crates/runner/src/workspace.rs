//! Workspace preparation: check out the session's repositories through the
//! git proxy and lay down skills where harnesses look for them.

use anyhow::{Context, Result, bail};
use hyperbeam_core::protocol::{RepoSpec, RunSpec, SkillSpec};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Environment that routes git through the control plane's proxy. No GitHub
/// credential ever enters the sandbox: the proxy holds it and checks every
/// fetch and push against the session's grants.
pub fn git_env(spec: &RunSpec, token: &str) -> BTreeMap<String, String> {
    let base = format!("{}/", spec.git_base_url.trim_end_matches('/'));
    let entries = [
        (format!("http.{base}.extraHeader"), format!("Authorization: Bearer {token}")),
        (format!("url.{base}.insteadOf"), "https://github.com/".to_string()),
        (format!("url.{base}.insteadOf"), "git@github.com:".to_string()),
        (format!("url.{base}.insteadOf"), "ssh://git@github.com/".to_string()),
        ("user.name".to_string(), spec.git_user_name.clone()),
        ("user.email".to_string(), spec.git_user_email.clone()),
        ("init.defaultBranch".to_string(), "main".to_string()),
        ("advice.detachedHead".to_string(), "false".to_string()),
    ];
    let mut env = BTreeMap::new();
    env.insert("GIT_CONFIG_COUNT".to_string(), entries.len().to_string());
    for (i, (key, value)) in entries.into_iter().enumerate() {
        env.insert(format!("GIT_CONFIG_KEY_{i}"), key);
        env.insert(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
    env
}

async fn git(cwd: &Path, env: &BTreeMap<String, String>, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .output()
        .await
        .context("could not run git (is it installed in the harness image?)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Clone one repository and put it on the session's work branch. Returns a
/// short description for the session log.
pub async fn checkout_repo(
    root: &Path,
    repo: &RepoSpec,
    work_branch: &str,
    env: &BTreeMap<String, String>,
) -> Result<String> {
    let dir = root.join(repo.dir_name());
    let url = format!("https://github.com/{}.git", repo.full_name);
    if !dir.join(".git").exists() {
        git(root, env, &["clone", "--quiet", &url, repo.dir_name()]).await?;
    }
    // A resumed session finds its earlier work on the remote.
    let remote_branch = git(&dir, env, &["ls-remote", "--heads", "origin", work_branch]).await?;
    if remote_branch.is_empty() {
        git(&dir, env, &["checkout", "--quiet", "-B", work_branch]).await?;
        Ok(format!("Checked out {} on new branch {work_branch}", repo.full_name))
    } else {
        git(&dir, env, &["fetch", "--quiet", "origin", work_branch]).await?;
        git(&dir, env, &["checkout", "--quiet", "-B", work_branch, "FETCH_HEAD"]).await?;
        Ok(format!("Checked out {} on existing branch {work_branch}", repo.full_name))
    }
}

fn skill_file(skill: &SkillSpec) -> String {
    if skill.content.trim_start().starts_with("---") {
        return skill.content.clone();
    }
    let description = skill.description.replace('\n', " ");
    format!("---\nname: {}\ndescription: {}\n---\n\n{}\n", skill.name, description, skill.content.trim_end())
}

/// Write skills in the Agent Skills layout. Both the Claude Code location and
/// the cross-harness `.agents` location are populated.
pub async fn write_skills(root: &Path, skills: &[SkillSpec]) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for skill in skills {
        // Names are validated server-side; refuse anything path-like anyway.
        if skill.name.contains(['/', '\\']) || skill.name.starts_with('.') {
            bail!("refusing to write skill with unsafe name `{}`", skill.name);
        }
        for base in [".claude/skills", ".agents/skills"] {
            let dir = root.join(base).join(&skill.name);
            tokio::fs::create_dir_all(&dir).await?;
            let path = dir.join("SKILL.md");
            tokio::fs::write(&path, skill_file(skill)).await?;
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_gets_frontmatter_once() {
        let plain = SkillSpec { name: "style".into(), description: "House\nstyle".into(), content: "Be terse.".into() };
        let file = skill_file(&plain);
        assert!(file.starts_with("---\nname: style\ndescription: House style\n---\n"));
        let already = SkillSpec { content: "---\nname: x\n---\nbody".into(), ..plain };
        assert_eq!(skill_file(&already), "---\nname: x\n---\nbody");
    }

    #[tokio::test]
    async fn unsafe_skill_names_are_refused() {
        let dir = std::env::temp_dir().join(format!("nm-skill-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = SkillSpec { name: "../x".into(), description: String::new(), content: String::new() };
        assert!(write_skills(&dir, &[bad]).await.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
