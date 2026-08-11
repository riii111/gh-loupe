mod support;

#[test]
fn public_pr_checks_behavior() {
    support::assert_shell_test_succeeds("pr_checks.sh");
}
