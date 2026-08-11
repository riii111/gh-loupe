---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、thread取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Version

最初に`gh-read --version`を実行し、インストール済みbinaryの版を確認する。
`--version`が`invalid choice`や`unrecognized arguments`で失敗する場合、インストール済みbinaryはこのoption追加前の版である。
その場合は現在のsourceからbinaryを再インストールする。
CLIとこのSkillの互換性に影響する変更では、将来の変更ごとにCargo package versionを上げる。
自動release processは前提にしない。

## Workflow

1. PR番号またはPR URLを確定し、`gh-read pr overview <番号またはURL> --compact`を実行する。
別repoなら`--repo OWNER/REPO`を加える。
2. `pullRequest`、required checkの集計、未解決review thread数から、review開始、CI待機、head更新への追従、追加取得のいずれが必要か判断する。
3. review decisionやreview本文が必要な場合だけ、`gh-read pr reviews <番号またはURL> --compact`でreview submissionを取得する。
   このsubcommandには`gh-read 0.2.1`以降が必要である。
4. threadの位置と件数が必要なら、`gh-read pr threads <番号またはURL> --compact`で本文を含まない一覧を取得する。
5. resolvedを含む一覧が必要な場合だけ`pr threads`へ`--include-resolved`を加える。既定は未解決threadだけである。
6. 一覧で特定したthreadのcomment本文が必要なら、`gh-read pr thread <番号またはURL> <thread ID> --compact`を実行する。`diffHunk`が必要な場合だけ`--include-diff-hunk`を加える。
7. 個別checkが必要なら`gh-read pr checks <番号またはURL> --compact`を使う。required checkだけなら`--required`を加える。
8. 失敗checkを調べるときは`--failed-diagnostics`でannotationを取得し、annotationだけで不足する場合だけ`--include-failed-logs`で制限付きlogを追加する。必要に応じて`--timeout SECONDS`を変更する。
9. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `pr overview`の`checks.required`、`checks.passed`、`checks.pending`、`checks.failed`はマージ要件であるrequired checkの集計である。`checks.all`はrequiredかどうかを問わないCI全体の活動状況を`total`、`passed`、`pending`、`failed`で表す。required checkが0件でも`checks.all`を確認する。
- `pr overview`の`data.checks`は`{"required": 数値, "passed": 数値, "pending": 数値, "failed": 数値, "all": {"total": 数値, "passed": 数値, "pending": 数値, "failed": 数値}}`の形である。既存のrequired側のfieldは維持される。
- `pr overview`の`reviewThreads.unresolved`は未解決threadの総数である。
- `pr reviews`はreview submissionだけを返す。conversation commentやreview thread commentとして扱わず、取得失敗を空配列と解釈しない。
- `pr threads`の`data.threads`が空なら、未解決threadはないと報告する。
- `pr thread`には`pr threads`が返したthread IDを渡し、取得失敗をcommentなしと解釈しない。
- `pr thread`で`diffHunk`が必要な場合だけ`--include-diff-hunk`を指定する。
- `pr checks`の`checks`が空ならCI情報がない状態として扱う。
- `pr checks`の`workflow`、`startedAt`、`completedAt`、`link`は`string | null`であり、通常経路と診断経路で同じ型である。GitHub CLIの空文字と時刻の`0001-01-01T00:00:00Z`は`null`を表す。pre-production段階のschema correctionのため、`schemaVersion`は1のままである。
- 診断の進捗はstderr、最終JSONはstdoutとして分けて扱う。`log: null`は対応するActions job logがないことを表し、取得失敗とは解釈しない。
- `truncated: true`のlogには省略がある。省略数が`null`なら正確な量を推測しない。
- 取得失敗を「コメントなし」と解釈しない。
- `pr threads`の構造化エラーでは`kind`と`retryable`を確認し、部分結果として扱わない。
- `pr thread`の構造化エラーでも`kind`と`retryable`を確認し、途中まで取得したcommentがあると推測しない。
