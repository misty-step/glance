use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceRepo {
    pub owner: String,
    pub name: String,
    pub sha: String,
}

impl SourceRepo {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn sister_name(&self) -> String {
        format!("{}-glance", self.name)
    }

    pub fn sister_slug(&self) -> String {
        format!("{}/{}", self.owner, self.sister_name())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PublishMode {
    Master,
    Branch { branch: String, pr_title: String },
}

#[derive(Debug, Clone)]
pub struct PublishRequest {
    pub site_dir: PathBuf,
    pub source: SourceRepo,
    pub worktree_dir: PathBuf,
    pub sister_remote: Option<String>,
    pub mode: PublishMode,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PublishOutcome {
    pub changed: bool,
    pub commit_sha: Option<String>,
    pub pushed_ref: String,
    pub pr_url: Option<String>,
    pub worktree_dir: PathBuf,
}

pub trait SisterHost {
    fn ensure_repo(&self, source: &SourceRepo) -> Result<String>;

    fn open_or_update_pr(
        &self,
        source: &SourceRepo,
        branch: &str,
        title: &str,
        body: &str,
    ) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct GhSisterHost;

impl SisterHost for GhSisterHost {
    fn ensure_repo(&self, source: &SourceRepo) -> Result<String> {
        let sister_slug = source.sister_slug();
        if run_status("gh", ["repo", "view", sister_slug.as_str()]).is_err() {
            run_status("gh", ["repo", "create", sister_slug.as_str(), "--private"])
                .with_context(|| format!("create GitHub repo {sister_slug}"))?;
        }
        Ok(format!("https://github.com/{sister_slug}.git"))
    }

    fn open_or_update_pr(
        &self,
        source: &SourceRepo,
        branch: &str,
        title: &str,
        body: &str,
    ) -> Result<String> {
        let sister_slug = source.sister_slug();
        if let Ok(url) = run_output(
            "gh",
            [
                "pr",
                "view",
                branch,
                "--repo",
                sister_slug.as_str(),
                "--json",
                "url",
                "--jq",
                ".url",
            ],
        ) {
            return Ok(url.trim().to_owned());
        }

        let url = run_output(
            "gh",
            [
                "pr",
                "create",
                "--repo",
                sister_slug.as_str(),
                "--head",
                branch,
                "--base",
                "master",
                "--title",
                title,
                "--body",
                body,
            ],
        )
        .with_context(|| format!("open sister PR for {sister_slug}:{branch}"))?;
        Ok(url.trim().to_owned())
    }
}

pub fn publish(request: PublishRequest, host: &impl SisterHost) -> Result<PublishOutcome> {
    validate_source(&request.source)?;
    let site_dir = request
        .site_dir
        .canonicalize()
        .with_context(|| format!("canonicalize site dir {}", request.site_dir.display()))?;
    validate_site_dir(&site_dir)?;

    let remote = match &request.sister_remote {
        Some(remote) => remote.clone(),
        None => host.ensure_repo(&request.source)?,
    };

    ensure_worktree(&remote, &request.worktree_dir)?;
    let target_ref = match &request.mode {
        PublishMode::Master => "master".to_owned(),
        PublishMode::Branch { branch, .. } => branch.clone(),
    };
    checkout_publish_ref(
        &request.worktree_dir,
        &target_ref,
        matches!(request.mode, PublishMode::Branch { .. }),
    )?;
    mirror_site(
        &site_dir,
        &request.worktree_dir,
        &request.source,
        &request.mode,
    )?;

    git(&request.worktree_dir, ["add", "-A"])?;
    let has_changes = !git_output(&request.worktree_dir, ["status", "--porcelain"])?
        .trim()
        .is_empty();

    if !has_changes {
        let pr_url = match &request.mode {
            PublishMode::Master => None,
            PublishMode::Branch {
                branch, pr_title, ..
            } if remote_branch_exists(&request.worktree_dir, branch)? => {
                Some(host.open_or_update_pr(
                    &request.source,
                    branch,
                    pr_title,
                    &pr_body(&request.source, &request.mode, request.run_id.as_deref()),
                )?)
            }
            PublishMode::Branch { .. } => None,
        };
        return Ok(PublishOutcome {
            changed: false,
            commit_sha: None,
            pushed_ref: target_ref,
            pr_url,
            worktree_dir: request.worktree_dir,
        });
    }

    let message = commit_message(&request.source, &request.mode, request.run_id.as_deref());
    git_with_config(
        &request.worktree_dir,
        ["commit", "-m", message.as_str()],
        [
            ("user.name", "glance publisher"),
            ("user.email", "glance-publisher@example.invalid"),
        ],
    )?;
    let commit_sha = git_output(&request.worktree_dir, ["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    git(
        &request.worktree_dir,
        ["push", "origin", format!("HEAD:{target_ref}").as_str()],
    )?;

    let pr_url = match &request.mode {
        PublishMode::Master => None,
        PublishMode::Branch { branch, pr_title } => Some(host.open_or_update_pr(
            &request.source,
            branch,
            pr_title,
            &pr_body(&request.source, &request.mode, request.run_id.as_deref()),
        )?),
    };

    Ok(PublishOutcome {
        changed: true,
        commit_sha: Some(commit_sha),
        pushed_ref: target_ref,
        pr_url,
        worktree_dir: request.worktree_dir,
    })
}

fn validate_source(source: &SourceRepo) -> Result<()> {
    for (name, value) in [
        ("source owner", source.owner.as_str()),
        ("source name", source.name.as_str()),
        ("source SHA", source.sha.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{name} is required");
        }
    }
    Ok(())
}

fn validate_site_dir(site_dir: &Path) -> Result<()> {
    if !site_dir.is_dir() {
        bail!("site dir is not a directory: {}", site_dir.display());
    }
    for file in generated_files(site_dir)? {
        let relative = file
            .strip_prefix(site_dir)
            .context("relative generated file")?;
        if !is_generated_payload(relative) {
            bail!(
                "site dir contains non-generated payload {}; only HTML and metadata files are publishable",
                relative.display()
            );
        }
    }
    Ok(())
}

fn ensure_worktree(remote: &str, worktree_dir: &Path) -> Result<()> {
    if worktree_dir.join(".git").is_dir() {
        git(worktree_dir, ["remote", "set-url", "origin", remote])?;
        git(worktree_dir, ["fetch", "origin", "--prune"])?;
        return Ok(());
    }
    if worktree_dir.exists() && worktree_dir.read_dir()?.next().is_some() {
        bail!(
            "sister worktree exists but is not a git checkout: {}",
            worktree_dir.display()
        );
    }
    if let Some(parent) = worktree_dir.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    run_status(
        "git",
        ["clone", remote, worktree_dir.display().to_string().as_str()],
    )
    .with_context(|| format!("clone sister remote {remote}"))?;
    Ok(())
}

fn checkout_publish_ref(
    worktree_dir: &Path,
    target_ref: &str,
    prefer_master_base: bool,
) -> Result<()> {
    if remote_branch_exists(worktree_dir, target_ref)? {
        git(
            worktree_dir,
            [
                "checkout",
                "-B",
                target_ref,
                format!("origin/{target_ref}").as_str(),
            ],
        )?;
    } else if prefer_master_base && remote_branch_exists(worktree_dir, "master")? {
        git(
            worktree_dir,
            ["checkout", "-B", target_ref, "origin/master"],
        )?;
    } else if git_succeeds(worktree_dir, ["rev-parse", "--verify", "HEAD"])? {
        git(worktree_dir, ["checkout", "-B", target_ref])?;
    } else {
        git(worktree_dir, ["checkout", "--orphan", target_ref])?;
    }
    Ok(())
}

fn mirror_site(
    site_dir: &Path,
    worktree_dir: &Path,
    source: &SourceRepo,
    mode: &PublishMode,
) -> Result<()> {
    clear_worktree_payload(worktree_dir)?;
    for source_file in generated_files(site_dir)? {
        let relative = source_file.strip_prefix(site_dir).with_context(|| {
            format!(
                "make {} relative to {}",
                source_file.display(),
                site_dir.display()
            )
        })?;
        let destination = worktree_dir.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::copy(&source_file, &destination).with_context(|| {
            format!(
                "copy generated payload {} to {}",
                source_file.display(),
                destination.display()
            )
        })?;
    }
    write_publish_metadata(worktree_dir, source, mode)?;
    Ok(())
}

fn clear_worktree_payload(worktree_dir: &Path) -> Result<()> {
    for entry in
        fs::read_dir(worktree_dir).with_context(|| format!("read {}", worktree_dir.display()))?
    {
        let entry = entry?;
        if entry.file_name() == OsStr::new(".git") {
            continue;
        }
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).with_context(|| format!("read {}", path.display()))?;
        if metadata.is_dir() {
            fs::remove_dir_all(&path).with_context(|| format!("remove {}", path.display()))?;
        } else {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
    }
    Ok(())
}

fn generated_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_generated_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_generated_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).with_context(|| format!("read {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("generated site contains symlink {}", path.display());
        }
        if metadata.is_dir() {
            collect_generated_files(&path, files)?;
        } else if metadata.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn is_generated_payload(relative: &Path) -> bool {
    let extension = relative.extension().and_then(OsStr::to_str);
    if matches!(extension, Some("html" | "json" | "toml")) {
        return true;
    }

    let filename = relative.file_name().and_then(OsStr::to_str).unwrap_or("");
    filename.starts_with("glance-image-")
        && matches!(extension, Some("png" | "jpg" | "jpeg" | "webp" | "svg"))
}

fn write_publish_metadata(
    worktree_dir: &Path,
    source: &SourceRepo,
    mode: &PublishMode,
) -> Result<()> {
    let metadata_dir = worktree_dir.join(".glance");
    fs::create_dir_all(&metadata_dir)
        .with_context(|| format!("create {}", metadata_dir.display()))?;
    fs::write(
        metadata_dir.join("source.toml"),
        publish_metadata(source, mode),
    )
    .context("write publish metadata")?;
    Ok(())
}

fn publish_metadata(source: &SourceRepo, mode: &PublishMode) -> String {
    let (mode_name, branch, title) = match mode {
        PublishMode::Master => ("master", "", ""),
        PublishMode::Branch { branch, pr_title } => ("branch", branch.as_str(), pr_title.as_str()),
    };
    format!(
        "source_owner = \"{}\"\nsource_name = \"{}\"\nsource_sha = \"{}\"\nmode = \"{}\"\nbranch = \"{}\"\nsource_pr_title = \"{}\"\n",
        toml_escape(&source.owner),
        toml_escape(&source.name),
        toml_escape(&source.sha),
        mode_name,
        toml_escape(branch),
        toml_escape(title)
    )
}

fn commit_message(source: &SourceRepo, mode: &PublishMode, run_id: Option<&str>) -> String {
    format!(
        "Publish glance site for {}@{}\n\nSource-Repo: {}\nSource-SHA: {}\nPublish-Mode: {}\nGlance-Run: {}\n",
        source.slug(),
        source.sha,
        source.slug(),
        source.sha,
        mode_name(mode),
        run_id.unwrap_or("unspecified")
    )
}

fn pr_body(source: &SourceRepo, mode: &PublishMode, run_id: Option<&str>) -> String {
    format!(
        "Mirrored glance site for `{}` at `{}`.\n\nSource SHA: `{}`\nPublish mode: `{}`\nRun: `{}`\n",
        source.slug(),
        source.sha,
        source.sha,
        mode_name(mode),
        run_id.unwrap_or("unspecified")
    )
}

fn mode_name(mode: &PublishMode) -> &'static str {
    match mode {
        PublishMode::Master => "master",
        PublishMode::Branch { .. } => "branch",
    }
}

fn remote_branch_exists(worktree_dir: &Path, branch: &str) -> Result<bool> {
    Ok(git(
        worktree_dir,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/remotes/origin/{branch}").as_str(),
        ],
    )
    .is_ok())
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn git<I, S>(dir: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let status = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(dir)
        .args(args.iter().map(AsRef::as_ref))
        .status()
        .with_context(|| format!("run git in {}", dir.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("git failed in {}", dir.display()))
    }
}

fn git_with_config<I, S, C, K, V>(dir: &Path, args: I, config: C) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    C: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(dir);
    for (key, value) in config {
        let mut item = OsString::from(key.as_ref());
        item.push("=");
        item.push(value.as_ref());
        command.arg("-c").arg(item);
    }
    let status = command
        .args(args.iter().map(AsRef::as_ref))
        .status()
        .with_context(|| format!("run git in {}", dir.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("git failed in {}", dir.display()))
    }
}

fn git_output<I, S>(dir: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(dir)
        .args(args.iter().map(AsRef::as_ref))
        .output()
        .with_context(|| format!("run git in {}", dir.display()))?;
    if !output.status.success() {
        bail!("git failed in {}", dir.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_succeeds<I, S>(dir: &Path, args: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(dir)
        .args(args.iter().map(AsRef::as_ref))
        .output()
        .with_context(|| format!("run git in {}", dir.display()))?;
    Ok(output.status.success())
}

fn run_status<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let status = Command::new(program)
        .args(args.iter().map(AsRef::as_ref))
        .status()
        .with_context(|| format!("run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{program} failed"))
    }
}

fn run_output<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new(program)
        .args(args.iter().map(AsRef::as_ref))
        .output()
        .with_context(|| format!("run {program}"))?;
    if !output.status.success() {
        bail!("{program} failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct FakeHost {
        remote: RefCell<Option<String>>,
        pr_url: String,
        opened: RefCell<Vec<String>>,
    }

    impl SisterHost for FakeHost {
        fn ensure_repo(&self, _source: &SourceRepo) -> Result<String> {
            self.remote
                .borrow()
                .clone()
                .context("fake remote not configured")
        }

        fn open_or_update_pr(
            &self,
            _source: &SourceRepo,
            branch: &str,
            title: &str,
            _body: &str,
        ) -> Result<String> {
            self.opened.borrow_mut().push(format!("{branch}:{title}"));
            Ok(self.pr_url.clone())
        }
    }

    #[test]
    fn publishes_to_local_bare_master_and_avoids_noop_commit() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = init_source_repo(temp.path())?;
        let site = temp.path().join("site");
        fs::create_dir_all(site.join("src/parser"))?;
        fs::write(site.join("index.html"), "<!doctype html><p>root</p>")?;
        fs::write(site.join("glance-image-001.png"), [137, 80, 78, 71])?;
        fs::write(
            site.join("src/parser/index.html"),
            "<!doctype html><p>parser</p>",
        )?;
        fs::write(site.join("metadata.json"), "{\"ok\":true}\n")?;
        let sister_bare = temp.path().join("demo-glance.git");
        git_init_bare(&sister_bare)?;
        let worktree = temp.path().join("demo-glance-worktree");
        let remote = file_url(&sister_bare);

        let host = FakeHost {
            remote: RefCell::new(Some(remote)),
            pr_url: "https://example.invalid/pr/1".to_owned(),
            opened: RefCell::default(),
        };
        let request = PublishRequest {
            site_dir: site,
            source: source.clone(),
            worktree_dir: worktree.clone(),
            sister_remote: None,
            mode: PublishMode::Master,
            run_id: Some("test-run".to_owned()),
        };

        let first = publish(request.clone(), &host)?;
        assert!(first.changed);
        assert!(first.commit_sha.is_some());
        assert_eq!(first.pushed_ref, "master");

        let first_remote_head = bare_head(&sister_bare, "master")?;
        let second = publish(request, &host)?;
        assert!(!second.changed);
        assert_eq!(second.commit_sha, None);
        assert_eq!(bare_head(&sister_bare, "master")?, first_remote_head);

        let inspect = temp.path().join("inspect");
        run_status(
            "git",
            [
                "clone",
                file_url(&sister_bare).as_str(),
                inspect.display().to_string().as_str(),
            ],
        )?;
        assert!(inspect.join("index.html").is_file());
        assert!(inspect.join("glance-image-001.png").is_file());
        assert!(inspect.join("src/parser/index.html").is_file());
        assert!(inspect.join("metadata.json").is_file());
        assert!(inspect.join(".glance/source.toml").is_file());
        assert!(!inspect.join("src/parser/mod.rs").exists());
        assert_only_generated_files(&inspect)?;

        let log = git_output(&inspect, ["log", "-1", "--pretty=%B"])?;
        assert!(log.contains("Source-SHA: "));
        assert!(log.contains("Glance-Run: test-run"));
        Ok(())
    }

    #[test]
    fn branch_mode_pushes_branch_and_returns_mocked_pr_url() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = init_source_repo(temp.path())?;
        let site = temp.path().join("branch-site");
        fs::create_dir_all(site.join("docs"))?;
        fs::write(site.join("docs/index.html"), "<!doctype html><p>docs</p>")?;
        let sister_bare = temp.path().join("branch-glance.git");
        git_init_bare(&sister_bare)?;
        let host = FakeHost {
            remote: RefCell::new(None),
            pr_url: "https://example.invalid/misty-step/demo-glance/pull/7".to_owned(),
            opened: RefCell::default(),
        };
        let request = PublishRequest {
            site_dir: site,
            source,
            worktree_dir: temp.path().join("branch-worktree"),
            sister_remote: Some(file_url(&sister_bare)),
            mode: PublishMode::Branch {
                branch: "glance/source-pr-12".to_owned(),
                pr_title: "Mirror source PR #12".to_owned(),
            },
            run_id: None,
        };

        let outcome = publish(request, &host)?;

        assert!(outcome.changed);
        assert_eq!(outcome.pushed_ref, "glance/source-pr-12");
        assert_eq!(
            outcome.pr_url.as_deref(),
            Some("https://example.invalid/misty-step/demo-glance/pull/7")
        );
        assert_eq!(
            host.opened.borrow().as_slice(),
            ["glance/source-pr-12:Mirror source PR #12"]
        );
        assert!(bare_head(&sister_bare, "glance/source-pr-12").is_ok());
        Ok(())
    }

    #[test]
    fn rejects_non_generated_payloads() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let site = temp.path().join("site");
        fs::create_dir_all(&site)?;
        fs::write(site.join("index.html"), "<!doctype html>")?;
        fs::write(site.join("lib.rs"), "fn main() {}\n")?;
        let host = FakeHost::default();
        let err = publish(
            PublishRequest {
                site_dir: site,
                source: SourceRepo {
                    owner: "misty-step".to_owned(),
                    name: "demo".to_owned(),
                    sha: "abc123".to_owned(),
                },
                worktree_dir: temp.path().join("worktree"),
                sister_remote: Some(file_url(&temp.path().join("missing.git"))),
                mode: PublishMode::Master,
                run_id: None,
            },
            &host,
        )
        .expect_err("non-generated payload should fail");

        assert!(err.to_string().contains("non-generated payload"));
        Ok(())
    }

    #[test]
    fn live_smoke_creates_github_repo_when_enabled() -> Result<()> {
        if std::env::var("GLANCE_LIVE_SMOKE").ok().as_deref() != Some("1") {
            return Ok(());
        }

        let temp = tempfile::tempdir()?;
        let site = temp.path().join("site");
        fs::create_dir_all(&site)?;
        fs::write(site.join("index.html"), "<!doctype html><p>live smoke</p>")?;
        let source = SourceRepo {
            owner: "misty-step".to_owned(),
            name: "glance-publish-smoke".to_owned(),
            sha: "live-smoke-sha".to_owned(),
        };
        let outcome = publish(
            PublishRequest {
                site_dir: site,
                source,
                worktree_dir: temp.path().join("glance-publish-smoke-glance"),
                sister_remote: None,
                mode: PublishMode::Master,
                run_id: Some("GLANCE_LIVE_SMOKE".to_owned()),
            },
            &GhSisterHost,
        )?;

        assert!(outcome.changed || outcome.commit_sha.is_none());
        Ok(())
    }

    fn init_source_repo(root: &Path) -> Result<SourceRepo> {
        let source_dir = root.join("demo-source");
        fs::create_dir_all(source_dir.join("src/parser"))?;
        fs::write(source_dir.join("src/parser/mod.rs"), "pub fn parse() {}\n")?;
        fs::write(source_dir.join("README.md"), "# demo\n")?;
        run_status(
            "git",
            [
                "init",
                "-b",
                "master",
                source_dir.display().to_string().as_str(),
            ],
        )?;
        git(&source_dir, ["add", "."])?;
        git_with_config(
            &source_dir,
            ["commit", "-m", "source fixture"],
            [
                ("user.name", "source fixture"),
                ("user.email", "source@example.invalid"),
            ],
        )?;
        let sha = git_output(&source_dir, ["rev-parse", "HEAD"])?
            .trim()
            .to_owned();
        Ok(SourceRepo {
            owner: "misty-step".to_owned(),
            name: "demo".to_owned(),
            sha,
        })
    }

    fn git_init_bare(path: &Path) -> Result<()> {
        run_status(
            "git",
            ["init", "--bare", path.display().to_string().as_str()],
        )
    }

    fn bare_head(path: &Path, branch: &str) -> Result<String> {
        run_output(
            "git",
            [
                "--git-dir",
                path.display().to_string().as_str(),
                "rev-parse",
                format!("refs/heads/{branch}").as_str(),
            ],
        )
        .map(|head| head.trim().to_owned())
    }

    fn file_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    fn assert_only_generated_files(root: &Path) -> Result<()> {
        for file in generated_files(root)? {
            let relative = file.strip_prefix(root)?;
            if relative
                .components()
                .next()
                .and_then(|component| component.as_os_str().to_str())
                == Some(".git")
            {
                continue;
            }
            assert!(
                is_generated_payload(relative) || relative == Path::new(".glance/source.toml"),
                "unexpected payload {}",
                relative.display()
            );
        }
        Ok(())
    }
}
