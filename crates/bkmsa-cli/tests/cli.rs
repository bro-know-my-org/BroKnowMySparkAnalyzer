use bkmsa_cli::{
    cli::{Cli, Command, OutputFormat},
    input::resolve_spark_report_url,
    parse_tool_args,
};
use clap::{CommandFactory, Parser};
use predicates::prelude::*;
use prost::Message;
use serde_json::json;

#[test]
fn clap_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn binary_and_subcommands_are_stable() {
    let cli = Cli::try_parse_from([
        "bkmsa",
        "inspect",
        "report.sparkprofile",
        "--format",
        "json",
    ])
    .expect("inspect command");
    let Command::Inspect(args) = cli.command else {
        panic!("expected inspect")
    };
    assert_eq!(args.source, "report.sparkprofile");
    assert_eq!(args.output.format, OutputFormat::Json);

    Cli::try_parse_from([
        "bkmsa",
        "tool",
        "report.sparkprofile",
        "hot-paths",
        "--category",
        "auto",
        "--limit",
        "20",
    ])
    .expect("tool convenience arguments");

    for command in ["tools", "inventory", "inspect", "tool", "analyze"] {
        let args = match command {
            "tools" => vec!["bkmsa", command],
            "tool" => vec!["bkmsa", command, "key", "overview"],
            _ => vec!["bkmsa", command, "key"],
        };
        Cli::try_parse_from(args).unwrap_or_else(|error| panic!("{command}: {error}"));
    }
}

#[test]
fn spark_viewer_urls_and_keys_resolve_to_content_origin() {
    assert_eq!(
        resolve_spark_report_url("abc123").unwrap(),
        "https://spark-usercontent.lucko.me/abc123"
    );
    assert_eq!(
        resolve_spark_report_url("https://spark.lucko.me/abc123").unwrap(),
        "https://spark-usercontent.lucko.me/abc123"
    );
    assert_eq!(
        resolve_spark_report_url("https://spark.lucko.me/viewer/?key=from-query").unwrap(),
        "https://spark-usercontent.lucko.me/from-query"
    );
    assert_eq!(
        resolve_spark_report_url("key with spaces").unwrap(),
        "https://spark-usercontent.lucko.me/key%20with%20spaces"
    );
    assert_eq!(
        resolve_spark_report_url("https://spark.lucko.me/key%20with%20spaces").unwrap(),
        "https://spark-usercontent.lucko.me/key%20with%20spaces"
    );
    assert!(resolve_spark_report_url("https://example.com/report").is_err());
}

#[test]
fn tool_args_accept_json_values_and_plain_strings() {
    assert_eq!(
        parse_tool_args(
            None,
            &[
                "limit=20".into(),
                "category=auto".into(),
                "enabled=true".into()
            ]
        )
        .unwrap(),
        json!({"limit": 20, "category": "auto", "enabled": true})
    );
    assert_eq!(
        parse_tool_args(Some(r#"{"limit":10}"#), &[]).unwrap(),
        json!({"limit": 10})
    );
}

#[test]
fn output_format_defaults_to_terminal() {
    let cli = Cli::try_parse_from(["bkmsa", "tools"]).unwrap();
    let Command::Tools(args) = cli.command else {
        panic!("expected tools")
    };
    assert_eq!(args.format, OutputFormat::Terminal);
}

#[test]
fn binary_help_names_commands_and_exit_codes() {
    let mut command = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    command.arg("--help");
    command.assert().success().stdout(
        predicate::str::contains("Usage: bkmsa")
            .and(predicate::str::contains("analyze"))
            .and(predicate::str::contains("Exit codes:")),
    );
}

#[test]
fn missing_local_report_uses_input_exit_code() {
    let mut command = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    command
        .args(["inspect", "definitely-missing.sparkprofile"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("file does not exist"));
}

#[test]
fn text_reports_are_first_class_cli_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let report = directory.path().join("server.log");
    std::fs::write(&report, "Can't keep up! Is the server overloaded?").unwrap();

    let mut command = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    let assertion = command
        .args(["inspect", report.to_str().unwrap(), "--format", "json"])
        .assert()
        .success();
    let output: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("valid JSON output");
    assert_eq!(output["kind"], "text");
}

#[test]
fn valid_sampler_protobuf_is_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let report = directory.path().join("valid.sparkprofile");
    let data = bkmsa_core::proto::SamplerData {
        metadata: Some(bkmsa_core::proto::SamplerMetadata {
            start_time: 123,
            ..Default::default()
        }),
        ..Default::default()
    };
    std::fs::write(&report, data.encode_to_vec()).unwrap();

    let mut command = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    let assertion = command
        .args(["inspect", report.to_str().unwrap(), "--format", "json"])
        .assert()
        .success();
    let output: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("valid JSON output");
    assert_eq!(output["kind"], "sampler");
}

#[test]
fn corrupt_protobuf_report_uses_decode_exit_code() {
    let directory = tempfile::tempdir().unwrap();
    let report = directory.path().join("corrupt.sparkprofile");
    std::fs::write(&report, "not protobuf").unwrap();

    let mut command = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    command
        .args(["inspect", report.to_str().unwrap()])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("protobuf"));
}

#[test]
fn text_stdin_requires_explicit_flag() {
    let mut rejected = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    rejected
        .args(["inspect", "-"])
        .write_stdin("plain text")
        .assert()
        .code(4);

    let mut accepted = assert_cmd::Command::cargo_bin("bkmsa").unwrap();
    accepted
        .args(["inspect", "-", "--text", "--format", "json"])
        .write_stdin("plain text")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\": \"text\""));
}
