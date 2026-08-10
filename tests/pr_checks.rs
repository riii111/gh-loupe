use std::path::PathBuf;
use std::process::Command;

#[test]
fn public_pr_checks_behavior() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(repository.join("tests/pr_checks.sh"))
        .current_dir(&repository)
        .env("GH_READ_BIN", env!("CARGO_BIN_EXE_gh-read"))
        .output()
        .expect("run pr checks tests");

    assert!(
        output.status.success(),
        "pr checks test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
