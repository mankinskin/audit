use assert_cmd::Command;
use rusqlite::Connection;
use audit_cli::cli::{
    CliOutput,
    parse_cli_from,
    run,
};
use tempfile::tempdir;

use super::fixtures::{
    assert_unix_formatted_output_text,
    assert_unix_formatted_output_value,
    write_sample_repo,
};

#[test]
fn cli_supports_json_and_text_output() {
    let repo = tempdir().expect("temp repo");
    write_sample_repo(repo.path());

    let cli = parse_cli_from([
        "audit",
        "run",
        repo.path().to_string_lossy().as_ref(),
        "--json",
        "--max-file-lines",
        "20",
        "--max-cyclomatic-complexity",
        "3",
    ])
    .expect("parse cli");

    match run(cli).expect("run cli") {
        CliOutput::Machine(
            value,
            audit_cli::cli::MachineOutputFormat::Json,
        ) => {
            assert_eq!(value["service"], "audit-mcp");
            assert!(
                value["findings"]
                    .as_array()
                    .is_some_and(|findings| !findings.is_empty())
            );
            assert_unix_formatted_output_value(&value["repo_root"]);
            assert_unix_formatted_output_value(&value["index_database"]);
            let compiler_warning = value["findings"]
                .as_array()
                .and_then(|findings| {
                    findings.iter().find(|finding| {
                        finding["category"] == "compiler_warning"
                    })
                })
                .expect("compiler warning finding");
            assert_unix_formatted_output_value(
                &compiler_warning["evidence"]["sample"][0]["path"],
            );
        },
        CliOutput::Machine(_, format) => {
            panic!("expected json machine output, got {format:?}");
        },
        CliOutput::Text(_) => panic!("expected json output"),
    }

    let text_cli = parse_cli_from([
        "audit",
        "run",
        repo.path().to_string_lossy().as_ref(),
        "--max-file-lines",
        "20",
        "--max-cyclomatic-complexity",
        "3",
    ])
    .expect("parse text cli");

    match run(text_cli).expect("run text cli") {
        CliOutput::Text(output) => assert_unix_formatted_output_text(&output),
        CliOutput::Machine(_, _) => panic!("expected text output"),
    }

    let mut command = Command::cargo_bin("audit").expect("audit binary");
    command
        .arg("run")
        .arg(repo.path())
        .arg("--max-file-lines")
        .arg("20")
        .arg("--max-cyclomatic-complexity")
        .arg("3");
    command
        .assert()
        .success()
        .stdout(predicates::str::contains("Repository Audit"));
}

#[test]
fn cli_dot_repo_root_reads_back_from_canonical_audit_store() {
    let dir = tempdir().expect("temp root");
    let selected = dir.path().join("selected");
    std::fs::create_dir_all(selected.join("src")).expect("create selected workspace");
    std::fs::write(selected.join("README.md"), "audit selector fixture\n")
        .expect("write source file");

    let output = Command::cargo_bin("audit")
        .expect("audit binary")
        .current_dir(&selected)
        .args(["run", ".", "--json"])
        .output()
        .expect("run audit CLI");
    assert!(
        output.status.success(),
        "audit CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse audit report");
    let run_id = report["run"]["run_id"]
        .as_i64()
        .expect("audit report contains run id");
    let canonical_store = selected.join(".workflow-tools").join("audit");
    let connection = Connection::open(canonical_store.join("audit.sqlite3"))
        .expect("open canonical audit database");
    let persisted: (String, String) = connection
        .query_row(
            "SELECT repo_root, status FROM audit_runs WHERE run_id = ?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read audit run by returned id");

    assert_eq!(persisted.0, report["repo_root"].as_str().unwrap());
    assert_eq!(persisted.1, "completed");
    assert!(!selected.join(".audit").exists());
    assert!(!dir.path().join(".workflow-tools").join("audit").exists());
}

#[test]
fn cli_without_subcommand_shows_help() {
    let mut command = Command::cargo_bin("audit").expect("audit binary");
    command
        .assert()
        .success()
        .stdout(predicates::str::contains("Repository quality audit CLI"))
        .stdout(predicates::str::contains("run"));
}

#[test]
fn cli_supports_toon_output() {
    let repo = tempdir().expect("temp repo");
    write_sample_repo(repo.path());

    let out = Command::cargo_bin("audit")
        .expect("audit binary")
        .arg("--toon")
        .arg("run")
        .arg(repo.path())
        .arg("--max-file-lines")
        .arg("20")
        .arg("--max-cyclomatic-complexity")
        .arg("3")
        .output()
        .expect("run audit with toon");

    assert!(
        out.status.success(),
        "audit --toon run failed ({})\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let rendered =
        String::from_utf8(out.stdout).expect("toon output should be utf-8");
    let parsed: serde_json::Value = toon_format::decode_default(&rendered)
        .expect("toon output should decode");

    assert_eq!(parsed["service"], "audit-mcp");
    assert!(
        parsed["findings"]
            .as_array()
            .is_some_and(|findings| !findings.is_empty())
    );
    assert_unix_formatted_output_value(&parsed["repo_root"]);
    assert_unix_formatted_output_value(&parsed["index_database"]);
}
