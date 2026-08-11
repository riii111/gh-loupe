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
    let previous_binary = binary_root.join("bin/gh-read");
    let previous_skill = skill_root.join("gh-read");
    fs::create_dir_all(previous_binary.parent().expect("binary parent"))
        .expect("create previous binary directory");
    fs::create_dir_all(&previous_skill).expect("create previous Skill");
    fs::create_dir(&temporary_root).expect("create temporary root");
    fs::write(&previous_binary, "obsolete binary").expect("write obsolete binary");
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
    assert_no_installer_work(&binary_root.join("bin"));
    assert_no_installer_work(&skill_root);
}

#[test]
fn build_failure_preserves_existing_destinations() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("failure");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let previous_binary = binary_root.join("bin/gh-read");
    let previous_skill_file = skill_root.join("gh-read/previous.md");
    let temporary_root = temporary.0.join("temporary files");
    let fake_cargo = temporary.0.join("fake cargo");
    fs::create_dir_all(previous_binary.parent().expect("binary parent"))
        .expect("create binary root");
    fs::create_dir_all(previous_skill_file.parent().expect("Skill parent"))
        .expect("create Skill root");
    fs::write(&previous_binary, "previous binary").expect("write previous binary");
    fs::write(&previous_skill_file, "previous Skill").expect("write previous Skill");
    fs::create_dir(&temporary_root).expect("create temporary root");
    write_executable(&fake_cargo, "#!/usr/bin/env bash\nexit 42\n");

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("CARGO", &fake_cargo)
        .env("TMPDIR", &temporary_root)
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
    assert_no_installer_work(&binary_root.join("bin"));
    assert_no_installer_work(&skill_root);
    assert!(directory_entries(&temporary_root).is_empty());
}

#[test]
fn missing_bundled_skill_fails_before_installation() {
    let source_repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("missing Skill");
    let repository = temporary.0.join("repository");
    let script = repository.join("install.sh");
    fs::create_dir_all(&repository).expect("create repository");
    fs::copy(source_repository.join("install.sh"), &script).expect("copy installer");

    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .output()
        .expect("run installer with missing Skill");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("bundled gh-read Skill is missing"));
    assert!(!binary_root.exists());
    assert!(!skill_root.exists());
}

#[test]
fn version_mismatch_fails_before_installation() {
    let source_repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("version mismatch");
    let repository = temporary.0.join("repository");
    let skill_path = repository.join("skills/gh-read/SKILL.md");
    fs::create_dir_all(skill_path.parent().expect("Skill parent")).expect("create Skill");
    fs::copy(
        source_repository.join("install.sh"),
        repository.join("install.sh"),
    )
    .expect("copy installer");
    fs::copy(
        source_repository.join("Cargo.toml"),
        repository.join("Cargo.toml"),
    )
    .expect("copy Cargo manifest");
    let skill = fs::read_to_string(source_repository.join("skills/gh-read/SKILL.md"))
        .expect("read bundled Skill");
    let skill = skill.replace(
        &format!("Required gh-read version: {}", env!("CARGO_PKG_VERSION")),
        "Required gh-read version: 9.9.9",
    );
    fs::write(&skill_path, skill).expect("write mismatched Skill");

    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let previous_binary = binary_root.join("bin/gh-read");
    let previous_skill = skill_root.join("gh-read/previous.md");
    fs::create_dir_all(previous_binary.parent().expect("binary parent"))
        .expect("create binary root");
    fs::create_dir_all(previous_skill.parent().expect("Skill destination parent"))
        .expect("create Skill root");
    fs::write(&previous_binary, "previous binary").expect("write previous binary");
    fs::write(&previous_skill, "previous Skill").expect("write previous Skill");

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .output()
        .expect("run installer with mismatched version");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Skill required version 9.9.9 does not match Cargo package version")
    );
    assert_eq!(
        fs::read_to_string(previous_binary).unwrap(),
        "previous binary"
    );
    assert_eq!(
        fs::read_to_string(previous_skill).unwrap(),
        "previous Skill"
    );
}

#[test]
fn binary_directory_symlink_fails_before_replacement() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("binary directory symlink");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let binary_destination = binary_root.join("bin/gh-read");
    let binary_target = temporary.0.join("binary target");
    let fake_cargo = temporary.0.join("fake cargo");
    fs::create_dir_all(binary_destination.parent().expect("binary parent"))
        .expect("create binary root");
    fs::create_dir(&binary_target).expect("create binary target");
    std::os::unix::fs::symlink(&binary_target, &binary_destination)
        .expect("create binary destination symlink");
    write_executable(&fake_cargo, "#!/usr/bin/env bash\nexit 42\n");

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("run installer with binary directory symlink");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("binary destination is a directory"));
    assert!(
        fs::symlink_metadata(&binary_destination)
            .expect("read binary destination metadata")
            .file_type()
            .is_symlink()
    );
    assert!(directory_entries(&binary_target).is_empty());
}

#[test]
fn later_temporary_directory_failure_removes_earlier_one() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("temporary failure");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let temporary_root = temporary.0.join("temporary files");
    let fake_tools = temporary.0.join("fake tools");
    let fake_mktemp = fake_tools.join("mktemp");
    let count_file = temporary.0.join("mktemp count");
    fs::create_dir(&temporary_root).expect("create temporary root");
    fs::create_dir(&fake_tools).expect("create fake tools directory");
    write_executable(
        &fake_mktemp,
        "#!/usr/bin/env bash\ncount=$(cat \"$MKTEMP_COUNT_FILE\" 2>/dev/null || echo 0)\nprintf '%s' \"$((count + 1))\" >\"$MKTEMP_COUNT_FILE\"\nif ((count == 0)); then\n  path=\"$TMPDIR/observed-cargo-work\"\n  mkdir \"$path\"\n  printf '%s\\n' \"$path\"\n  exit 0\nfi\nexit 43\n",
    );

    let mut search_paths = vec![fake_tools];
    search_paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH is set"),
    ));
    let search_path = std::env::join_paths(search_paths).expect("join PATH");
    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("PATH", search_path)
        .env("TMPDIR", &temporary_root)
        .env("MKTEMP_COUNT_FILE", count_file)
        .output()
        .expect("run installer with failing mktemp");

    assert_eq!(output.status.code(), Some(43));
    assert!(!temporary_root.join("observed-cargo-work").exists());
    assert_no_installer_work(&binary_root.join("bin"));
    assert_no_installer_work(&skill_root);
}

#[test]
fn staging_validation_failure_preserves_existing_destinations() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDirectory::new("staging failure");
    let binary_root = temporary.0.join("binary root");
    let skill_root = temporary.0.join("skill root");
    let temporary_root = temporary.0.join("temporary files");
    let previous_binary = binary_root.join("bin/gh-read");
    let previous_skill_file = skill_root.join("gh-read/previous.md");
    let fake_cargo = temporary.0.join("fake cargo");
    fs::create_dir_all(previous_binary.parent().expect("binary parent"))
        .expect("create binary root");
    fs::create_dir_all(previous_skill_file.parent().expect("Skill parent"))
        .expect("create Skill root");
    fs::create_dir(&temporary_root).expect("create temporary root");
    fs::write(&previous_binary, "previous binary").expect("write previous binary");
    fs::write(&previous_skill_file, "previous Skill").expect("write previous Skill");
    write_executable(
        &fake_cargo,
        "#!/usr/bin/env bash\nroot=\nwhile (($# > 0)); do\n  if [[ $1 == --root ]]; then\n    root=$2\n    shift 2\n  else\n    shift\n  fi\ndone\nmkdir -p \"$root/bin\"\ncat >\"$root/bin/gh-read\" <<'EOF'\n#!/usr/bin/env bash\nprintf '%s\\n' 'gh-read invalid'\nEOF\nchmod +x \"$root/bin/gh-read\"\n",
    );

    let output = Command::new("bash")
        .arg(repository.join("install.sh"))
        .arg("--binary-root")
        .arg(&binary_root)
        .arg("--skill-root")
        .arg(&skill_root)
        .env("CARGO", &fake_cargo)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("run installer with invalid staged version");

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(&previous_binary).unwrap(),
        "previous binary"
    );
    assert_eq!(
        fs::read_to_string(&previous_skill_file).unwrap(),
        "previous Skill"
    );
    assert_no_installer_work(&binary_root.join("bin"));
    assert_no_installer_work(&skill_root);
    assert!(directory_entries(&temporary_root).is_empty());
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

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write executable");
    let mut permissions = fs::metadata(path)
        .expect("read executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make file executable");
}

fn assert_no_installer_work(directory: &Path) {
    assert!(
        directory_entries(directory)
            .iter()
            .all(|entry| !entry.to_string_lossy().starts_with(".gh-read.install.")),
        "installer work directory remains in {}",
        directory.display()
    );
}
