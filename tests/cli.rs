mod support;

use std::path::PathBuf;
use std::process::Command;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::fd::OwnedFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::process::Stdio;

#[test]
fn validates_public_cli_behavior() {
    support::assert_shell_test_succeeds("cli.sh");
}

#[cfg(unix)]
#[test]
fn closed_stdout_does_not_panic_for_help_or_normal_output() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary_root = std::env::temp_dir().join(format!(
        "gh-loupe-closed-stdout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos()
    ));
    let bin = temporary_root.join("bin");
    fs::create_dir_all(&bin).expect("create CLI fixture directory");
    let fixture = bin.join("gh");
    fs::copy(repository.join("tests/fixtures/gh"), &fixture).expect("copy gh fixture");
    let mut permissions = fs::metadata(&fixture)
        .expect("read gh fixture metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fixture, permissions).expect("make gh fixture executable");

    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH is set"),
    ));
    let path = std::env::join_paths(paths).expect("join PATH");

    for arguments in [["--help"].as_slice(), ["issue", "42"].as_slice()] {
        let (reader, writer) = UnixStream::pair().expect("create stdout socket pair");
        drop(reader);
        let writer: OwnedFd = writer.into();
        let output = Command::new(env!("CARGO_BIN_EXE_gh-loupe"))
            .args(arguments)
            .env("PATH", &path)
            .stdout(Stdio::from(writer))
            .output()
            .expect("run CLI with closed stdout");
        assert!(
            output.status.success(),
            "CLI failed with closed stdout\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("panicked"), "unexpected panic: {stderr}");
        assert!(
            !stderr.contains("stack backtrace"),
            "unexpected backtrace: {stderr}"
        );
    }

    fs::remove_dir_all(temporary_root).expect("remove CLI fixture directory");
}
