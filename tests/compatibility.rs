use std::path::PathBuf;
use std::process::Command;

#[test]
fn matches_python_reference() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(repository.join("tests/compatibility.sh"))
        .current_dir(&repository)
        .env("GH_READ_BIN", env!("CARGO_BIN_EXE_gh-read"))
        .output()
        .expect("run compatibility tests");

    assert!(
        output.status.success(),
        "compatibility test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
