mod support;

#[test]
fn public_pr_files_behavior() {
    support::assert_shell_test_succeeds("pr_files.sh");
}
