---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、comments、reviews、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、コメント取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Compatibility

最初に`gh-read --version`を実行し、インストール済みbinaryの版を確認する。
`--version`が`invalid choice`や`unrecognized arguments`で失敗する場合、インストール済みbinaryはこのoption追加前の版である。
その場合は現在のsourceからbinaryを再インストールする。
CLIとこのSkillの互換性に影響する変更では、将来の変更ごとにCargo package versionを上げる。
自動release processは前提にしない。

## Workflow

1. PR番号またはPR URLを確定し、`gh-read pr overview <番号またはURL> --compact`を実行する。
別repoなら`--repo OWNER/REPO`を加える。
2. `pullRequest`、required checkの集計、未解決review thread数から、review開始、CI待機、head更新への追従、追加取得のいずれが必要か判断する。
3. threadの位置と件数が必要なら、`gh-read pr threads <番号またはURL> --compact`で本文を含まない一覧を取得する。
4. resolvedを含む一覧が必要な場合だけ`pr threads`へ`--include-resolved`を加える。既定は未解決threadだけである。
5. 一覧で特定したthreadのcomment本文が必要なら、`gh-read pr thread <番号またはURL> <thread ID> --compact`を実行する。`diffHunk`が必要な場合だけ`--include-diff-hunk`を加える。
6. 個別checkが必要なら`gh-read pr checks <番号またはURL> --compact`を使う。required checkだけなら`--required`を加える。
7. 失敗checkを調べるときは`--failed-diagnostics`でannotationを取得し、annotationだけで不足する場合だけ`--include-failed-logs`で制限付きlogを追加する。必要に応じて`--timeout SECONDS`を変更する。
8. PR本文、conversation comments、review本文を含む既存形式が必要な場合だけ`gh-read pr <番号またはURL> --compact`を実行する。
9. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。
10. 既存の`pr`で`diffHunk`が必要な場合だけ`--compact`を外す。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `pr overview`の`checks`はrequired checkの集計であり、`reviewThreads.unresolved`は未解決threadの総数である。
- `pr threads`の`data.threads`が空なら、未解決threadはないと報告する。
- `pr thread`には`pr threads`が返したthread IDを渡し、取得失敗をcommentなしと解釈しない。
- `pr thread`で`diffHunk`が必要な場合だけ`--include-diff-hunk`を指定する。
- 既存の`pr`で`reviewThreads`が空なら、未解決threadはないと報告する。
- `pr checks`の`checks`が空ならCI情報がない状態として扱う。
- 診断の進捗はstderr、最終JSONはstdoutとして分けて扱う。`log: null`は対応するActions job logがないことを表し、取得失敗とは解釈しない。
- `truncated: true`のlogには省略がある。省略数が`null`なら正確な量を推測しない。
- 取得失敗を「コメントなし」と解釈しない。
- `pr threads`の構造化エラーでは`kind`と`retryable`を確認し、部分結果として扱わない。
- `pr thread`の構造化エラーでも`kind`と`retryable`を確認し、途中まで取得したcommentがあると推測しない。
