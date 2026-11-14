//! Workspace preparation: check out the session's repositories through the
//! git proxy and lay down skills where harnesses look for them.

use anyhow::{Context, Result, bail};
use hyperbeam_core::protocol::{HARNESS_DIR, HOME_SKIPPED_MARKER, RepoSpec, RunSpec, SkillSpec};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// How long a repository's setup command may take.
const SETUP_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How much of a setup command's output the session log keeps.
const SETUP_OUTPUT_CHARS: usize = 3000;

/// The ways a repository under `host_url` can be addressed: over HTTPS and,
/// for the same host name, over SSH.
fn host_prefixes(host_url: &str) -> Vec<String> {
    let web = format!("{}/", host_url.trim_end_matches('/'));
    let mut prefixes = vec![web.clone()];
    let rest = web.split_once("://").map(|(_, rest)| rest).unwrap_or(&web);
    if let Some((authority, path)) = rest.split_once('/') {
        let name = authority.rsplit('@').next().unwrap_or(authority);
        let name = name.split(':').next().unwrap_or(name);
        if !name.is_empty() {
            prefixes.push(format!("git@{name}:{path}"));
            prefixes.push(format!("ssh://git@{name}/{path}"));
        }
    }
    prefixes
}

/// Environment that routes git through the control plane's proxy. No code
/// host credential ever enters the sandbox: the proxy holds it and checks
/// every fetch and push against the session's grants.
pub fn git_env(spec: &RunSpec, token: &str) -> BTreeMap<String, String> {
    let base = format!("{}/", spec.git_base_url.trim_end_matches('/'));
    let mut hosts = vec!["https://github.com".to_string()];
    for repo in &spec.repos {
        if !hosts.iter().any(|h| h.trim_end_matches('/') == repo.host_url.trim_end_matches('/')) {
            hosts.push(repo.host_url.clone());
        }
    }
    let mut entries = vec![(format!("http.{base}.extraHeader"), format!("Authorization: Bearer {token}"))];
    for host in &hosts {
        for prefix in host_prefixes(host) {
            entries.push((format!("url.{base}.insteadOf"), prefix));
        }
    }
    entries.extend([
        ("user.name".to_string(), spec.git_user_name.clone()),
        ("user.email".to_string(), spec.git_user_email.clone()),
        ("init.defaultBranch".to_string(), "main".to_string()),
        ("advice.detachedHead".to_string(), "false".to_string()),
    ]);
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
    let url = repo.clone_url();
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

/// Run a repository's setup command in its checkout. Returns a line for the
/// session log either way: a setup that fails leaves the agent to work in
/// what there is, as a person would.
pub async fn run_setup(root: &Path, repo: &RepoSpec, env: &BTreeMap<String, String>) -> String {
    let started = Instant::now();
    let name = &repo.full_name;
    // One pipe for both streams keeps the output in the order it was written.
    let script = format!("exec 2>&1\n{}", repo.setup_command);
    let spawned = Command::new("sh")
        .args(["-c", &script])
        .current_dir(root.join(repo.dir_name()))
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => return format!("Setup for {name} could not start: {err}"),
    };
    let mut stdout = child.stdout.take();
    let run = async {
        let mut output = Vec::new();
        if let Some(stdout) = &mut stdout {
            stdout.read_to_end(&mut output).await.ok();
        }
        (child.wait().await, output)
    };
    let took = |started: Instant| format!("{}s", started.elapsed().as_secs());
    match tokio::time::timeout(SETUP_TIMEOUT, run).await {
        Ok((Ok(status), _)) if status.success() => format!("Setup for {name} finished in {}", took(started)),
        Ok((status, output)) => {
            let how = match status {
                Ok(status) => status.to_string(),
                Err(err) => err.to_string(),
            };
            format!("Setup for {name} failed after {} ({how}). The agent starts anyway.{}", took(started), tail(&output))
        }
        Err(_) => format!("Setup for {name} was stopped after {} minutes. The agent starts anyway.", SETUP_TIMEOUT.as_secs() / 60),
    }
}

/// The end of a command's output, where the error usually is.
fn tail(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let text = text.trim_end();
    if text.is_empty() {
        return String::new();
    }
    let skip = text.chars().count().saturating_sub(SETUP_OUTPUT_CHARS);
    let kept: String = text.chars().skip(skip).collect();
    format!("\n\n{}{kept}", if skip > 0 { "…" } else { "" })
}

/// What a setup command gets of the harness's environment: the route out
/// through the egress proxy, and not the harness credential.
pub fn proxy_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .filter(|(key, _)| key.to_ascii_lowercase().ends_with("_proxy") || key.as_str() == "NODE_USE_ENV_PROXY")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Where the agent of a harness image lives when it is there to be found: the
/// launcher for `program`, and the `PATH` the agent should run with.
pub fn harness_command(program: &str, path: Option<&str>) -> (String, Option<String>) {
    resolve_harness(Path::new(HARNESS_DIR), program, path)
}

fn resolve_harness(dir: &Path, program: &str, path: Option<&str>) -> (String, Option<String>) {
    let bin = dir.join("bin");
    if !bin.is_dir() {
        return (program.to_string(), None);
    }
    let launcher = bin.join(program);
    let program = if !program.contains('/') && launcher.is_file() { launcher.to_string_lossy().into_owned() } else { program.to_string() };
    // Last on the path: what the image provides comes first, so that the
    // agent's own `node` never stands in for the project's.
    let mut parts: Vec<String> = path.unwrap_or_default().split(':').filter(|part| !part.is_empty()).map(str::to_string).collect();
    for extra in [bin, dir.join("libexec")] {
        let extra = extra.to_string_lossy().into_owned();
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    (program, Some(parts.join(":")))
}

/// Was the agent meant to be carried in from a harness image that has none?
pub fn harness_missing() -> bool {
    std::fs::read_dir(HARNESS_DIR).is_ok_and(|mut entries| entries.next().is_none())
}

/// If the image's home directory was too big to carry into the sandbox's,
/// say so once, in words someone can act on.
pub fn home_skipped() -> Option<String> {
    let marker = Path::new(&std::env::var_os("HOME")?).join(HOME_SKIPPED_MARKER);
    let bytes: u64 = std::fs::read_to_string(&marker).ok()?.trim().parse().ok()?;
    std::fs::remove_file(&marker).ok();
    Some(format!(
        "The image keeps {:.1} GB in its home directory, more than a sandbox's home can take, so the sandbox started with an empty one. \
         Install toolchains outside the home directory, under /usr/local or /opt.",
        bytes as f64 / 1e9
    ))
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
    fn hosts_are_addressed_over_https_and_ssh() {
        assert_eq!(host_prefixes("https://github.com"), ["https://github.com/", "git@github.com:", "ssh://git@github.com/"]);
        assert_eq!(
            host_prefixes("https://git.acme.dev:8443/gitlab/"),
            ["https://git.acme.dev:8443/gitlab/", "git@git.acme.dev:gitlab/", "ssh://git@git.acme.dev/gitlab/"]
        );
    }

    #[test]
    fn the_agent_of_a_harness_image_is_found_and_kept_last_on_the_path() {
        let dir = std::env::temp_dir().join(format!("hb-harness-{}", uuid::Uuid::new_v4()));
        // No agent directory: the command and path are left alone.
        assert_eq!(resolve_harness(&dir, "my-agent", Some("/usr/bin")), ("my-agent".to_string(), None));

        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/my-agent"), "").unwrap();
        let (bin, libexec) = (dir.join("bin").to_string_lossy().into_owned(), dir.join("libexec").to_string_lossy().into_owned());
        assert_eq!(
            resolve_harness(&dir, "my-agent", Some("/usr/local/bin:/usr/bin")),
            (format!("{bin}/my-agent"), Some(format!("/usr/local/bin:/usr/bin:{bin}:{libexec}")))
        );
        // Something the image provides itself, or a path, is not second-guessed.
        assert_eq!(resolve_harness(&dir, "other", Some(&bin)).0, "other");
        assert_eq!(resolve_harness(&dir, "/usr/bin/my-agent", None).0, "/usr/bin/my-agent");
        assert_eq!(resolve_harness(&dir, "other", Some(&bin)).1, Some(format!("{bin}:{libexec}")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn setup_runs_in_the_checkout_and_reports_how_it_went() {
        let root = std::env::temp_dir().join(format!("hb-setup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("api")).unwrap();
        let repo = |command: &str| RepoSpec {
            full_name: "acme/api".into(),
            default_branch: "main".into(),
            host_url: "https://github.com".into(),
            setup_command: command.into(),
        };
        let env = BTreeMap::from([("GREETING".to_string(), "hello".to_string())]);
        let note = run_setup(&root, &repo("echo $GREETING > made && test \"$(basename $PWD)\" = api"), &env).await;
        assert!(note.starts_with("Setup for acme/api finished in "), "{note}");
        assert_eq!(std::fs::read_to_string(root.join("api/made")).unwrap(), "hello\n");

        let note = run_setup(&root, &repo("echo out; echo broken >&2; exit 3"), &env).await;
        assert!(note.starts_with("Setup for acme/api failed after "), "{note}");
        assert!(note.contains("exit status: 3") && note.ends_with("\n\nout\nbroken"), "{note}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn setup_gets_the_proxy_but_not_the_credential() {
        let env = BTreeMap::from([
            ("ANTHROPIC_API_KEY".to_string(), "secret".to_string()),
            ("HTTPS_PROXY".to_string(), "http://proxy".to_string()),
            ("no_proxy".to_string(), "localhost".to_string()),
            ("NODE_USE_ENV_PROXY".to_string(), "1".to_string()),
        ]);
        let kept: Vec<String> = proxy_env(&env).into_keys().collect();
        assert_eq!(kept, ["HTTPS_PROXY", "NODE_USE_ENV_PROXY", "no_proxy"]);
    }

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
