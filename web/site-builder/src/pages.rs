//! Publish locally built files without a second checkout or a CI compilation.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
type Error = Box<dyn std::error::Error>;
fn output(command: &mut Command) -> Result<String, Error> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    command
}
struct TemporaryIndex(PathBuf);
impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(PathBuf::from(format!("{}.lock", self.0.display())));
    }
}
pub fn publish(root: &Path, dist: &Path) -> Result<(), Error> {
    if !output(git(root).args(["status", "--porcelain"]))?.is_empty() {
        return Err("Commit source changes before publishing Pages".into());
    }
    output(git(root).args(["fetch", "origin", "main"]))?;
    output(git(root).args(["merge-base", "--is-ancestor", "HEAD", "origin/main"]))?;
    let source = output(git(root).args(["rev-parse", "HEAD"]))?;
    let repository = output(Command::new("gh").current_dir(root).args([
        "repo",
        "view",
        "--json",
        "nameWithOwner",
        "--jq",
        ".nameWithOwner",
    ]))?;
    let endpoint = format!("repos/{repository}/pages");
    let configuration = Command::new("gh")
        .args([
            "api",
            &endpoint,
            "--jq",
            ".source.branch + \":\" + .source.path",
        ])
        .output()?;
    let configured = if configuration.status.success() {
        if String::from_utf8_lossy(&configuration.stdout).trim() != "gh-pages:/" {
            return Err("An existing Pages site uses a different source; left unchanged".into());
        }
        true
    } else if String::from_utf8_lossy(&configuration.stderr).contains("HTTP 404") {
        false
    } else {
        return Err(String::from_utf8_lossy(&configuration.stderr)
            .into_owned()
            .into());
    };
    let remote = output(git(root).args(["ls-remote", "--heads", "origin", "refs/heads/gh-pages"]))?;
    let parent = remote.split_whitespace().next();
    if let Some(parent) = parent {
        output(git(root).args(["fetch", "origin", parent]))?;
    }
    fs::write(dist.join(".nojekyll"), [])?;
    fs::write(dist.join("source-revision.txt"), format!("{source}\n"))?;
    let git_dir = output(git(root).args(["rev-parse", "--absolute-git-dir"]))?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let index = TemporaryIndex(std::env::temp_dir().join(format!(
        "synthphone-pages-{}-{nonce}.index",
        std::process::id()
    )));
    let staged = || {
        let mut command = Command::new("git");
        command
            .current_dir(dist)
            .env("GIT_DIR", &git_dir)
            .env("GIT_WORK_TREE", dist)
            .env("GIT_INDEX_FILE", &index.0);
        command
    };
    output(staged().args(["read-tree", "--empty"]))?;
    // Publish only the runtime bundle, never unrelated files left in dist.
    output(staged().args([
        "add",
        "--force",
        "--",
        ".nojekyll",
        "source-revision.txt",
        "index.html",
        "lab.html",
        "phone.css",
        "worker.js",
        "audio-worklet.js",
        "engine.js",
        "engine_bg.wasm",
        "phone.js",
        "phone_bg.wasm",
        "synthphone.bin",
        "assets/phone-base.webp",
        "assets/phone-on-hook.webp",
        "assets/receiver.webp",
        "assets/wood.webp",
    ]))?;
    let tree = output(staged().arg("write-tree"))?;
    let mut commit = git(root);
    commit.args(["commit-tree", &tree]);
    if let Some(parent) = parent {
        commit.args(["-p", parent]);
    }
    commit.args(["-m", &format!("Publish Synthphone from {source}")]);
    let commit = output(&mut commit)?;
    output(git(root).args(["push", "origin", &format!("{commit}:refs/heads/gh-pages")]))?;
    if !configured {
        let mut child = Command::new("gh")
            .args(["api", "--method", "POST", &endpoint, "--input", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("Missing API input pipe")?
            .write_all(br#"{"build_type":"legacy","source":{"branch":"gh-pages","path":"/"}}"#)?;
        let response = child.wait_with_output()?;
        if !response.status.success() {
            // GitHub can enable branch-based Pages while processing the push.
            // Accept that race only after verifying the exact requested source.
            let source = output(Command::new("gh").args([
                "api",
                &endpoint,
                "--jq",
                ".source.branch + \":\" + .source.path",
            ]))?;
            if source != "gh-pages:/" {
                return Err(format!(
                    "Pages files pushed, but enabling the requested source failed: {}",
                    String::from_utf8_lossy(&response.stderr)
                )
                .into());
            }
        }
    }
    let url = output(Command::new("gh").args(["api", &endpoint, "--jq", ".html_url"]))?;
    println!("Pushed {repository} gh-pages at {commit}; source {source}\nGitHub Pages: {url}");
    Ok(())
}
