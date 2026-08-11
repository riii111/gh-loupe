use std::path::PathBuf;
use std::process::Command;

#[test]
fn public_pr_reviews_behavior() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(repository.join("tests/pr_reviews.sh"))
        .current_dir(&repository)
        .env("GH_READ_BIN", env!("CARGO_BIN_EXE_gh-read"))
        .output()
        .expect("run pr reviews tests");

    assert!(
        output.status.success(),
        "pr reviews test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
