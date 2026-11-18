use std::process::Command;

#[test]
fn rejects_noninteractive_output_before_loading_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_things3"))
        .args(["tui"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires an interactive terminal"));
}

#[test]
fn rejects_json_and_exposes_list_options_in_help() {
    let binary = env!("CARGO_BIN_EXE_things3");
    let output = Command::new(binary)
        .args(["tui", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not support --json"));
    let help = Command::new(binary)
        .args(["tui", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("projects"));
}
