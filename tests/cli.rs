use std::path::PathBuf;
use std::process::Command;

#[test]
fn validates_public_cli_behavior() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(repository.join("tests/cli.sh"))
        .current_dir(&repository)
        .env("GH_READ_BIN", env!("CARGO_BIN_EXE_gh-read"))
        .output()
        .expect("run CLI tests");

    assert!(
        output.status.success(),
        "CLI test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
