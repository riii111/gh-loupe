mod support;

#[test]
fn public_pr_reviews_behavior() {
    support::assert_shell_test_succeeds("pr_reviews.sh");
}
