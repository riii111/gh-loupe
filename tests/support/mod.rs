use std::path::PathBuf;
use std::process::Command;

pub fn assert_shell_test_succeeds(script: &str) {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(repository.join("tests").join(script))
        .current_dir(&repository)
        .env("GH_LOUPE_BIN", env!("CARGO_BIN_EXE_gh-loupe"))
        .output()
        .unwrap_or_else(|error| panic!("run shell test {script}: {error}"));

    assert!(
        output.status.success(),
        "shell test {script} failed with process status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
