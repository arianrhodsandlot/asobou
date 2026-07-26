use std::process::Command;

fn run(args: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .args(args)
        .output()
        .expect("failed to run asoby");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, code)
}

#[test]
fn prints_arguments() {
    let (out, code) = run(&["hello", "world"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "hello world");
}

#[test]
fn no_newline_flag() {
    let (out, code) = run(&["-n", "hello"]);
    assert_eq!(code, 0);
    assert_eq!(out, "hello");
}

#[test]
fn empty_args_produces_blank_line() {
    let (out, code) = run(&[]);
    assert_eq!(code, 0);
    assert_eq!(out, "\n");
}
