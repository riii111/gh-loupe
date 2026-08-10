---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、comments、reviews、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、コメント取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Workflow

1. PR番号またはPR URLを確定し、`gh-read pr <番号またはURL> --compact`を実行する。別repoなら`--repo OWNER/REPO`を加える。
2. JSONの`pullRequest`、`checks`、`conversationComments`、`reviews`、`reviewThreads`を確認する。
3. checkの一覧だけが必要なら`gh-read pr checks <番号またはURL> --compact`を使う。required checkだけなら`--required`を加える。
4. 失敗checkを調べるときは`--failed-diagnostics`でannotationを取得し、annotationだけで不足する場合だけ`--include-failed-logs`で制限付きlogを追加する。必要に応じて`--timeout SECONDS`を変更する。
5. resolvedを含む全review threadが必要な場合だけ`--include-resolved`を加える。`reviewThreads`は既定で未解決threadだけである。
6. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。
7. `diffHunk`が必要な場合だけ`--compact`を外す。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `reviewThreads`が空なら、未解決threadはないと報告する。`checks`が空ならCI情報がない状態として扱う。
- 診断の進捗はstderr、最終JSONはstdoutとして分けて扱う。`log: null`はActions jobではないことを表し、取得失敗とは解釈しない。
- `truncated: true`のlogには省略がある。省略数が`null`なら正確な量を推測しない。
- 取得失敗を「コメントなし」と解釈しない。
