---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、comments、reviews、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、コメント取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Workflow

1. PR番号またはPR URLを確定し、`gh-read pr <番号またはURL> --compact`を実行する。別repoなら`--repo OWNER/REPO`を加える。
2. JSONの`pullRequest`、`checks`、`conversationComments`、`reviews`、`reviewThreads`を確認する。
3. resolvedを含む全review threadが必要な場合だけ`--include-resolved`を加える。`reviewThreads`は既定で未解決threadだけである。
4. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。
5. `diffHunk`が必要な場合だけ`--compact`を外す。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `reviewThreads`が空なら、未解決threadはないと報告する。`checks`が空ならCI情報がない状態として扱う。
- 取得失敗を「コメントなし」と解釈しない。
