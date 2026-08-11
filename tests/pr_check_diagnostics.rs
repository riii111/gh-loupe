mod support;

#[test]
fn public_failed_check_diagnostics_behavior() {
    support::assert_shell_test_succeeds("pr_check_diagnostics.sh");
}
