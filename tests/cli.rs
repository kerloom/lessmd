use std::process::{Command, Output};

fn lessmd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lessmd"))
        .args(args)
        .output()
        .expect("failed to run lessmd")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_prints_usage_and_exits_successfully() {
    let output = lessmd(&["--help"]);

    assert!(output.status.success());
    let stdout = stdout(&output);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("--no-syntax"));
    assert!(stdout.contains("--no-mermaid"));
    assert!(stderr(&output).is_empty());
}

#[test]
fn version_prints_package_version_and_exits_successfully() {
    let output = lessmd(&["--version"]);

    assert!(output.status.success());
    assert_eq!(
        stdout(&output),
        format!("lessmd {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(stderr(&output).is_empty());
}

#[test]
fn unknown_option_exits_with_usage_error() {
    let output = lessmd(&["--does-not-exist"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("lessmd: unknown option: --does-not-exist"));
}

#[test]
fn source_read_error_exits_before_terminal_setup() {
    let output = lessmd(&["tests/fixtures/__missing_lessmd_cli_fixture__.md"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).starts_with("lessmd: "));
}
