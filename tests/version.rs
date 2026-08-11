use std::process::Command;

#[test]
fn version_reports_the_cargo_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_gh-read"))
        .arg("--version")
        .output()
        .expect("run gh-read --version");

    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).as_bytes()
    );
    assert!(output.stderr.is_empty());
}
