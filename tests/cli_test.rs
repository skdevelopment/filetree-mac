use std::process::Command;

fn filetree_bin() -> String {
    env!("CARGO_BIN_EXE_filetree-mac").to_string()
}

#[test]
fn test_cli_version() {
    let output = Command::new(filetree_bin())
        .arg("--version")
        .output()
        .expect("run filetree --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("filetree"));
}

#[test]
fn test_cli_help() {
    let output = Command::new(filetree_bin())
        .arg("--help")
        .output()
        .expect("run filetree --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Path to scan"));
}
