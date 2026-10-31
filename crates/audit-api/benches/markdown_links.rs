//! Criterion benchmark for [`audit_api::trials::markdown_links::evaluate`].

use audit_api::trials::markdown_links::evaluate;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Creates a synthetic repository structure containing guidance files and background code files.
fn create_synthetic_workspace(guidance_file_count: usize, background_file_count: usize) -> TempDir {
    let temp = TempDir::new().expect("create temp dir");
    let root = temp.path();

    // Create root AGENTS.md and README.md
    fs::write(
        root.join("AGENTS.md"),
        "# Root Agents\nSee [.agents/instructions/core.instructions.md](.agents/instructions/core.instructions.md)\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "# Root README\n").unwrap();

    let agents_dir = root.join(".agents/instructions");
    fs::create_dir_all(&agents_dir).unwrap();

    // Create core instruction file
    fs::write(
        agents_dir.join("core.instructions.md"),
        "# Core\nSee [../agents/worker.agent.md](../agents/worker.agent.md)\n",
    )
    .unwrap();

    let agents_templates = root.join(".agents/agents");
    fs::create_dir_all(&agents_templates).unwrap();
    fs::write(
        agents_templates.join("worker.agent.md"),
        "# Worker\nLink to [../../AGENTS.md](../../AGENTS.md)\n",
    )
    .unwrap();

    // Create multiple guidance files with various links
    for i in 0..guidance_file_count {
        let file_path = agents_dir.join(format!("guide_{i}.instructions.md"));
        let mut content = format!("# Guide {i}\n\n");
        // Internal valid links
        content.push_str("See [core](core.instructions.md) for details.\n");
        content.push_str("Refer to [root](../../AGENTS.md) overview.\n");
        // Non-guidance links
        content.push_str("Check [code](../../src/lib.rs) for implementation.\n");
        // External links
        content.push_str("Docs at [web](https://example.com/docs).\n");
        // Anchor links
        content.push_str("Jump to [section](#section-heading).\n");

        for j in 0..10 {
            content.push_str(&format!(
                "Line {j} with text and [link](guide_{i}.instructions.md#anchor)\n"
            ));
        }
        fs::write(file_path, content).unwrap();
    }

    // Create background files in non-guidance directories (src, target, node_modules)
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "// lib.rs\n").unwrap();

    let target_dir = root.join("target/debug/build");
    fs::create_dir_all(&target_dir).unwrap();

    let node_modules_dir = root.join("node_modules/pkg");
    fs::create_dir_all(&node_modules_dir).unwrap();

    for i in 0..background_file_count {
        fs::write(
            src_dir.join(format!("module_{i}.rs")),
            format!("// Rust source file {i}\n"),
        )
        .unwrap();

        if i % 2 == 0 {
            fs::write(
                target_dir.join(format!("artifact_{i}.o")),
                format!("// object file {i}\n"),
            )
            .unwrap();
        }

        if i % 5 == 0 {
            fs::write(
                node_modules_dir.join(format!("package_{i}.js")),
                format!("// JS file {i}\n"),
            )
            .unwrap();
        }
    }

    temp
}

/// Runs `git` in `dir`, panicking on failure (bench fixture setup only).
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=Bench",
            "-c",
            "user.email=bench@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Root repository with a committed submodule `sub`; root documents link into
/// the submodule (with anchors), so evaluation reads blobs at the gitlink.
fn create_submodule_workspace(doc_count: usize) -> (TempDir, TempDir) {
    let sub_src = TempDir::new().expect("create submodule source");
    git(sub_src.path(), &["init", "-q", "-b", "main"]);
    fs::create_dir_all(sub_src.path().join("docs")).unwrap();
    for i in 0..doc_count {
        fs::write(
            sub_src.path().join(format!("docs/d{i}.md")),
            format!("# Doc {i}\n\n## Section\n\nSee [self](#section).\n"),
        )
        .unwrap();
    }
    git(sub_src.path(), &["add", "-A"]);
    git(sub_src.path(), &["commit", "-q", "-m", "init"]);

    let root = TempDir::new().expect("create root");
    git(root.path(), &["init", "-q", "-b", "main"]);
    let mut readme = String::from("# Root\n\n");
    for i in 0..doc_count {
        readme.push_str(&format!(
            "[d{i}](sub/docs/d{i}.md#section) [h{i}](sub/docs/d{i}.md#doc-{i})\n"
        ));
    }
    fs::write(root.path().join("README.md"), readme).unwrap();
    git(root.path(), &["add", "-A"]);
    git(root.path(), &["commit", "-q", "-m", "init"]);
    git(
        root.path(),
        &[
            "submodule",
            "add",
            "-q",
            sub_src.path().to_str().unwrap(),
            "sub",
        ],
    );
    git(root.path(), &["commit", "-q", "-m", "add sub"]);
    (root, sub_src)
}

fn bench_markdown_links_evaluate(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_links_evaluate");

    // Submodule fixture: links and anchors resolved at the gitlink revision.
    for &doc_count in &[10usize, 50] {
        let (root, _sub_src) = create_submodule_workspace(doc_count);
        let repo_root = root.path().to_path_buf();
        group.bench_with_input(
            BenchmarkId::new("submodule_workspace", format!("docs{doc_count}")),
            &repo_root,
            |b, path| {
                b.iter(|| {
                    let result = evaluate(path, &[]);
                    assert_eq!(result.metric.blocking_findings, 0);
                    assert!(result.metric.links_checked >= doc_count * 2);
                });
            },
        );
    }

    // Scenarios varying background file count to measure traversal / pruning overhead
    for &(guidance_count, bg_count) in &[(10, 50), (25, 200), (50, 500)] {
        let fixture = create_synthetic_workspace(guidance_count, bg_count);
        let repo_root = fixture.path().to_path_buf();

        group.bench_with_input(
            BenchmarkId::new(
                "synthetic_workspace",
                format!("g{guidance_count}_bg{bg_count}"),
            ),
            &repo_root,
            |b, path| {
                b.iter(|| {
                    let result = evaluate(path, &[]);
                    assert!(result.metric.markdown_files >= guidance_count);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_markdown_links_evaluate);
criterion_main!(benches);
