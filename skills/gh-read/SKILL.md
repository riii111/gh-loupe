---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、comments、reviews、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、コメント取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Workflow

1. PR番号またはPR URLを確定する。別repoなら`--repo OWNER/REPO`を加える。
2. threadの有無と位置だけが必要なら、`gh-read pr threads <番号またはURL> --compact`で本文を含まない一覧を取得する。
3. resolvedを含む一覧が必要な場合だけ`pr threads`へ`--include-resolved`を加える。既定は未解決threadだけである。
4. PR本文、checks、conversation comments、reviews、各threadのcomment本文まで必要なら、`gh-read pr <番号またはURL> --compact`を実行する。
5. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。
6. 既存`pr`出力で`diffHunk`が必要な場合だけ`--compact`を外す。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `pr threads`の`data.threads`が空なら、未解決threadはないと報告する。既存`pr`の`reviewThreads`が空の場合も同様に扱う。`checks`が空ならCI情報がない状態として扱う。
- 取得失敗を「コメントなし」と解釈しない。
- `pr threads`の構造化エラーでは`kind`と`retryable`を確認し、部分結果として扱わない。
