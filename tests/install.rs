use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(case: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gh-read-install-test-{case}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

#[test]
fn installs_binary_and_replaces_the_bundled_skill() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("path with spaces");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let temporary_root = temporary.0.join("temporary files");
    let previous_skill = skill_root.join("gh-read");
    fs::create_dir_all(&previous_skill).expect("create previous Skill");
    fs::create_dir(&temporary_root).expect("create temporary root");
    fs::write(previous_skill.join("obsolete.md"), "obsolete").expect("write obsolete file");

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("run installer");

    assert!(
        output.status.success(),
        "installer failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("installer stdout is UTF-8");
    let installed_binary = binary_root.join("bin/gh-read");
    let displayed_binary = fs::canonicalize(&binary_root)
        .expect("canonicalize binary root")
        .join("bin/gh-read");
    let displayed_skill = fs::canonicalize(&skill_root)
        .expect("canonicalize Skill root")
        .join("gh-read");
    assert!(stdout.contains(&format!(
        "Binary destination: {}",
        displayed_binary.display()
    )));
    assert!(stdout.contains(&format!("Skill destination: {}", displayed_skill.display())));
    assert!(!previous_skill.join("obsolete.md").exists());
    assert_directories_equal(&repository.join("skills/gh-read"), &previous_skill);
    assert!(
        fs::read_dir(&temporary_root)
            .expect("read temporary root")
            .next()
            .is_none()
    );

    let version = Command::new(installed_binary)
        .arg("--version")
        .output()
        .expect("run installed binary");
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        format!("gh-read {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
}

#[test]
fn build_failure_preserves_existing_destinations() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("failure");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let previous_binary = binary_root.join("bin/gh-read");
    let previous_skill_file = skill_root.join("gh-read/previous.md");
    let fake_cargo = temporary.0.join("fake cargo");
    fs::create_dir_all(previous_binary.parent().expect("binary parent"))
        .expect("create binary root");
    fs::create_dir_all(previous_skill_file.parent().expect("Skill parent"))
        .expect("create Skill root");
    fs::write(&previous_binary, "previous binary").expect("write previous binary");
    fs::write(&previous_skill_file, "previous Skill").expect("write previous Skill");
    fs::write(&fake_cargo, "#!/usr/bin/env bash\nexit 42\n").expect("write fake cargo");
    let mut permissions = fs::metadata(&fake_cargo)
        .expect("read fake cargo metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_cargo, permissions).expect("make fake cargo executable");

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("run installer with failing cargo");

    assert_eq!(output.status.code(), Some(42));
    assert_eq!(
        fs::read_to_string(previous_binary).unwrap(),
        "previous binary"
    );
    assert_eq!(
        fs::read_to_string(previous_skill_file).unwrap(),
        "previous Skill"
    );
}

#[test]
fn skill_minimum_version_matches_the_cargo_package_version() {
    let skill =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/gh-read/SKILL.md"))
            .expect("read bundled Skill");
    let marker = format!("Minimum gh-read version: {}", env!("CARGO_PKG_VERSION"));
    assert!(skill.lines().any(|line| line == marker));
}

fn assert_directories_equal(expected: &Path, actual: &Path) {
    let mut expected_entries = directory_entries(expected);
    let mut actual_entries = directory_entries(actual);
    expected_entries.sort();
    actual_entries.sort();
    assert_eq!(expected_entries, actual_entries);

    for entry in expected_entries {
        let expected_path = expected.join(&entry);
        let actual_path = actual.join(&entry);
        if expected_path.is_dir() {
            assert!(
                actual_path.is_dir(),
                "{} is not a directory",
                actual_path.display()
            );
            assert_directories_equal(&expected_path, &actual_path);
        } else {
            assert_eq!(
                fs::read(&expected_path).expect("read expected file"),
                fs::read(&actual_path).expect("read actual file"),
                "file differs: {}",
                entry.display()
            );
        }
    }
}

fn directory_entries(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("read directory")
        .map(|entry| PathBuf::from(entry.expect("read directory entry").file_name()))
        .collect()
}
