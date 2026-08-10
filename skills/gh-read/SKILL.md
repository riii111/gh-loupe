---
name: gh-read
description: GitHubのPR/Issue metadata、head/base SHA、Draft/state、CI/checks、comments、reviews、review threadsを読み取り専用wrapperで取得する。PRやIssueの確認、CI調査、コメント取得、自律的なレビューで使う。
---

# GitHub read

`gh-read`を使い、API queryをその場で組み立てずにGitHubの定型情報を取得する。

## Workflow

1. PR番号またはPR URLを確定し、`gh-read pr overview <番号またはURL> --compact`を実行する。
別repoなら`--repo OWNER/REPO`を加える。
2. `pullRequest`、required checkの集計、未解決review thread数から、review開始、CI待機、head更新への追従、追加取得のいずれが必要か判断する。
3. 個別checkが必要なら`gh-read pr checks <番号またはURL> --compact`を使う。required checkだけなら`--required`を加える。
4. 失敗checkを調べるときは`--failed-diagnostics`でannotationを取得し、annotationだけで不足する場合だけ`--include-failed-logs`で制限付きlogを追加する。必要に応じて`--timeout SECONDS`を変更する。
5. PR本文、conversation comments、review本文、thread本文が必要な場合だけ`gh-read pr <番号またはURL> --compact`を実行する。
6. resolvedを含む全review threadが必要な場合だけ既存の`pr`へ`--include-resolved`を加える。
7. Issueは`gh-read issue <番号またはURL> --compact`で取得し、`issue`と`comments`を確認する。
8. 既存の`pr`で`diffHunk`が必要な場合だけ`--compact`を外す。

## Rules

- GitHubの定型読み取りは`gh-read`を使い、直接`gh api`を組み立てない。
- `gh-read`にないraw endpoint、任意GraphQL、任意jq/queryで代用しない。
- コメント取得中にreply、resolve、dismiss、editを行わない。
- `pr overview`の`checks`はrequired checkの集計であり、`reviewThreads.unresolved`は未解決threadの総数である。
- 既存の`pr`で`reviewThreads`が空なら、未解決threadはないと報告する。
- `pr checks`の`checks`が空ならCI情報がない状態として扱う。
- 診断の進捗はstderr、最終JSONはstdoutとして分けて扱う。`log: null`は対応するActions job logがないことを表し、取得失敗とは解釈しない。
- `truncated: true`のlogには省略がある。省略数が`null`なら正確な量を推測しない。
- 取得失敗を「コメントなし」と解釈しない。
