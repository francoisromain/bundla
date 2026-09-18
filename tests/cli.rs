use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bundla"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

#[test]
fn help_exits_zero() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bundle src into dist"), "got: {stdout}");
}

#[test]
fn version_exits_zero() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "got: {stdout}");
}

#[test]
fn release_open_without_serve_fails() {
    let out = run(&["--release", "--open"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--open requires a running server"),
        "got: {stderr}"
    );
}

#[test]
fn release_header_without_serve_fails() {
    let out = run(&["--release", "--header", "X-Engine: bundla"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--header requires a running server"),
        "got: {stderr}"
    );
}

#[test]
fn invalid_port_rejected() {
    let out = run(&["--port", "99999"]);
    assert_eq!(out.status.code(), Some(2), "clap parse errors exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid value") || stderr.contains("error:"),
        "got: {stderr}"
    );
}
