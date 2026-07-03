use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use glance_check::CitationChecker;
use glance_core::{RegenerationPlan, SourcePin, leaf_to_root_dirs, snapshot_tree};
use glance_gen::{
    GenerationConfig, GenerationRequest, MockProvider, PageGenerator, PageKind, PageSpend,
    ProviderMode, RealPageGenerator, SpendReport, spend_report_lines,
};
use glance_publish::{GhSisterHost, PublishRequest, SourceRepo};
use serde::Deserialize;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Generate and check citation-backed glance sites"
)]
struct Cli {
    #[arg(long, default_value = "glance.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    Plan {
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long = "changed")]
        changed_paths: Vec<PathBuf>,
    },
    Check {
        #[arg(long)]
        source_root: Option<PathBuf>,
        #[arg(long)]
        source_sha: Option<String>,
        html: Vec<PathBuf>,
    },
    ServeLocal {
        #[arg(long)]
        site_root: Option<PathBuf>,
        #[arg(long, default_value_t = 4173)]
        port: u16,
        #[arg(long)]
        once: bool,
    },
    Publish {
        #[arg(long)]
        site_dir: PathBuf,
        #[arg(long)]
        source_owner: String,
        #[arg(long)]
        source_name: String,
        #[arg(long)]
        source_sha: String,
        #[arg(long, value_enum)]
        mode: PublishModeArg,
        #[arg(long)]
        sister_worktree: Option<PathBuf>,
        #[arg(long)]
        sister_remote: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        source_pr_title: Option<String>,
        #[arg(long)]
        run_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PublishModeArg {
    Branch,
    Master,
}

#[derive(Debug, Default, Deserialize)]
struct GlanceConfig {
    source_root: Option<PathBuf>,
    site_root: Option<PathBuf>,
    source_sha: Option<String>,
    changed_paths: Option<Vec<PathBuf>>,
    #[serde(default)]
    generation: GenerationConfig,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;

    match cli.command {
        Command::Run { root } => run_command(&config, root),
        Command::Plan {
            root,
            changed_paths,
        } => plan_command(&config, root, changed_paths),
        Command::Check {
            source_root,
            source_sha,
            html,
        } => check_command(&config, source_root, source_sha, html),
        Command::ServeLocal {
            site_root,
            port,
            once,
        } => serve_local_command(&config, site_root, port, once),
        Command::Publish {
            site_dir,
            source_owner,
            source_name,
            source_sha,
            mode,
            sister_worktree,
            sister_remote,
            branch,
            source_pr_title,
            run_id,
        } => publish_command(PublishCommand {
            site_dir,
            source_owner,
            source_name,
            source_sha,
            mode,
            sister_worktree,
            sister_remote,
            branch,
            source_pr_title,
            run_id,
        }),
    }
}

#[derive(Debug)]
struct PublishCommand {
    site_dir: PathBuf,
    source_owner: String,
    source_name: String,
    source_sha: String,
    mode: PublishModeArg,
    sister_worktree: Option<PathBuf>,
    sister_remote: Option<String>,
    branch: Option<String>,
    source_pr_title: Option<String>,
    run_id: Option<String>,
}

fn run_command(config: &GlanceConfig, root: Option<PathBuf>) -> Result<()> {
    let root = root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let source_sha = configured_or_git_sha(config, &root)?;
    let snapshot = snapshot_tree(&root, &source_sha)?;
    let generation = config.generation.clone();
    let routing = generation.routing.clone();
    let provider: Box<dyn PageGenerator> = match generation.provider_mode {
        ProviderMode::Mock => Box::new(MockProvider::with_routing(routing.clone())),
        ProviderMode::Real => Box::new(RealPageGenerator::from_env(generation)?),
    };
    let mut spend_report = SpendReport::default();

    println!("source_sha={source_sha}");
    println!("directories={}", snapshot.directories.len());
    for directory in leaf_to_root_dirs(snapshot.directories.keys().cloned()) {
        let kind = if directory == Path::new(".") {
            PageKind::Root
        } else {
            let record = snapshot.directory(&directory).context("directory record")?;
            if record.child_dirs.is_empty() {
                PageKind::Leaf
            } else {
                PageKind::Interior
            }
        };
        let route = routing.model_for(kind);
        let page = provider.generate(GenerationRequest {
            source_root: snapshot.source_root.clone(),
            directory: directory.clone(),
            source_sha: source_sha.clone(),
            kind,
        })?;
        spend_report.record(PageSpend {
            directory: directory.clone(),
            provider: page.provider.clone(),
            model: page.model.clone(),
            input_tokens: page.input_tokens,
            output_tokens: page.output_tokens,
            spend_micros: page.spend_micros,
        });
        println!(
            "would_generate={} kind={:?} tier={:?} provider={} model={} max_tokens={} input_tokens={} output_tokens={} spend_micros={}",
            directory.display(),
            kind,
            page.tier,
            page.provider,
            page.model,
            route.max_tokens,
            page.input_tokens,
            page.output_tokens,
            page.spend_micros
        );
        for note in page.metadata_notes {
            println!("metadata_note={} {}", directory.display(), note);
        }
    }
    for line in spend_report_lines(&spend_report) {
        println!("{line}");
    }
    Ok(())
}

fn plan_command(
    config: &GlanceConfig,
    root: Option<PathBuf>,
    changed_paths: Vec<PathBuf>,
) -> Result<()> {
    let root = root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let changed_paths = if changed_paths.is_empty() {
        config.changed_paths.clone().unwrap_or_default()
    } else {
        changed_paths
    };

    if changed_paths.is_empty() {
        let source_sha =
            configured_or_git_sha(config, &root).unwrap_or_else(|_| "WORKTREE".to_owned());
        let snapshot = snapshot_tree(&root, source_sha)?;
        for directory in leaf_to_root_dirs(snapshot.directories.keys().cloned()) {
            println!("{}", directory.display());
        }
        return Ok(());
    }

    let plan = RegenerationPlan::from_changed_paths(&root, changed_paths)?;
    for directory in plan.directories {
        println!("{}", directory.display());
    }
    Ok(())
}

fn check_command(
    config: &GlanceConfig,
    source_root: Option<PathBuf>,
    source_sha: Option<String>,
    html: Vec<PathBuf>,
) -> Result<()> {
    let source_root = source_root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let source_sha = match source_sha.or_else(|| config.source_sha.clone()) {
        Some(source_sha) => source_sha,
        None => configured_or_git_sha(config, &source_root)?,
    };
    let html_files = if html.is_empty() {
        let site_root = config
            .site_root
            .clone()
            .context("no HTML files supplied and no site_root in config")?;
        find_html_files(&site_root)?
    } else {
        html
    };

    let checker = CitationChecker::new(&source_root, source_sha);
    let mut total_citations = 0;
    let mut total_failures = 0;

    for html_file in html_files {
        let report = checker.check_html_file(&html_file)?;
        total_citations += report.citations_checked;
        total_failures += report.failures.len();
        if report.is_ok() {
            println!(
                "ok {} citations={}",
                html_file.display(),
                report.citations_checked
            );
        } else {
            println!(
                "fail {} citations={} failures={}",
                html_file.display(),
                report.citations_checked,
                report.failures.len()
            );
            for failure in report.failures {
                println!(
                    "  {}:{}-{} {}",
                    failure.citation.path.display(),
                    failure.citation.start_line,
                    failure.citation.end_line,
                    failure.message
                );
            }
        }
    }

    if total_failures > 0 {
        bail!("{total_failures} broken citations across {total_citations} checked citations");
    }

    println!("checked {total_citations} citations");
    Ok(())
}

fn serve_local_command(
    config: &GlanceConfig,
    site_root: Option<PathBuf>,
    port: u16,
    once: bool,
) -> Result<()> {
    let site_root = site_root
        .or_else(|| config.site_root.clone())
        .unwrap_or_else(|| PathBuf::from("site"));
    let site_root = site_root
        .canonicalize()
        .with_context(|| format!("canonicalize site root {}", site_root.display()))?;
    let listener = TcpListener::bind(("127.0.0.1", port)).context("bind local server")?;
    let address = listener.local_addr().context("local address")?;
    println!("serving {} at http://{address}", site_root.display());

    for stream in listener.incoming() {
        handle_connection(stream.context("incoming connection")?, &site_root)?;
        if once {
            break;
        }
    }
    Ok(())
}

fn publish_command(command: PublishCommand) -> Result<()> {
    let source = SourceRepo {
        owner: command.source_owner,
        name: command.source_name,
        sha: command.source_sha,
    };
    let worktree_dir = command.sister_worktree.unwrap_or_else(|| {
        PathBuf::from("target")
            .join("glance-publish")
            .join(source.sister_name())
    });
    let mode = match command.mode {
        PublishModeArg::Master => glance_publish::PublishMode::Master,
        PublishModeArg::Branch => {
            let branch = command
                .branch
                .unwrap_or_else(|| format!("glance/{}", short_sha(&source.sha)));
            let pr_title = command
                .source_pr_title
                .context("--source-pr-title is required for --mode branch")?;
            glance_publish::PublishMode::Branch { branch, pr_title }
        }
    };

    let outcome = glance_publish::publish(
        PublishRequest {
            site_dir: command.site_dir,
            source,
            worktree_dir,
            sister_remote: command.sister_remote,
            mode,
            run_id: command.run_id,
        },
        &GhSisterHost,
    )?;

    println!("changed={}", outcome.changed);
    println!("sister_ref={}", outcome.pushed_ref);
    println!("worktree={}", outcome.worktree_dir.display());
    if let Some(commit_sha) = outcome.commit_sha {
        println!("commit_sha={commit_sha}");
    }
    if let Some(pr_url) = outcome.pr_url {
        println!("pr_url={pr_url}");
    }

    Ok(())
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

fn handle_connection(mut stream: TcpStream, site_root: &Path) -> Result<()> {
    let mut buffer = [0; 1024];
    let read = stream.read(&mut buffer).context("read request")?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let relative = match request_path_to_relative(path) {
        Some(relative) => relative,
        None => {
            write_response(&mut stream, "403 Forbidden", b"forbidden")?;
            return Ok(());
        }
    };
    let mut file_path = if relative.as_os_str().is_empty() {
        site_root.join("index.html")
    } else {
        site_root.join(relative)
    };
    if file_path.is_dir() {
        file_path = file_path.join("index.html");
    }

    match file_path.canonicalize() {
        Ok(canonical) if canonical.starts_with(site_root) => {
            let bytes = std::fs::read(canonical).context("read served file")?;
            write_response(&mut stream, "200 OK", &bytes)?;
        }
        Ok(_) => write_response(&mut stream, "403 Forbidden", b"forbidden")?,
        Err(_) => write_response(&mut stream, "404 Not Found", b"not found")?,
    }
    Ok(())
}

fn request_path_to_relative(path: &str) -> Option<PathBuf> {
    let path = path
        .split('?')
        .next()
        .unwrap_or(path)
        .trim_start_matches('/');
    let mut relative = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(relative)
}

fn write_response(stream: &mut TcpStream, status: &str, body: &[u8]) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .context("write header")?;
    stream.write_all(body).context("write body")?;
    Ok(())
}

fn load_config(path: &Path) -> Result<GlanceConfig> {
    if !path.exists() {
        return Ok(GlanceConfig::default());
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parse config {}", path.display()))
}

fn configured_or_git_sha(config: &GlanceConfig, root: &Path) -> Result<String> {
    if let Some(source_sha) = &config.source_sha {
        Ok(source_sha.clone())
    } else {
        Ok(SourcePin::resolve_git_head(root)?.sha)
    }
}

fn find_html_files(root: &Path) -> Result<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("canonicalize HTML root {}", root.display()))?;
    let mut files = Vec::new();
    collect_html_files(&root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_html_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("read metadata {}", path.display()))?;
        if metadata.file_type().is_dir() {
            collect_html_files(&path, files)?;
        } else if metadata.file_type().is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("html")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_paths_reject_parent_traversal() {
        assert!(request_path_to_relative("/../secret").is_none());
        assert_eq!(
            request_path_to_relative("/nested/index.html?x=1").expect("relative"),
            PathBuf::from("nested/index.html")
        );
    }

    #[cfg(unix)]
    #[test]
    fn html_scan_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let site = tempfile::tempdir().expect("site");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(site.path().join("index.html"), "<html></html>").expect("site html");
        std::fs::write(outside.path().join("outside.html"), "<html></html>").expect("outside html");
        symlink(outside.path(), site.path().join("linked")).expect("symlink");

        let files = find_html_files(site.path()).expect("files");

        assert_eq!(
            files,
            vec![site.path().join("index.html").canonicalize().unwrap()]
        );
    }
}
