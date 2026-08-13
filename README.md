# gh-loupe

GitHubの定型的な読み取りを安全に行い、AI Agentに渡す情報量を抑えるread-only CLIです。

<!-- Documentation boundary: README explains value and entry points; the Skill explains retrieval choices; command and schema details belong in docs/reference.md and --help. Do not duplicate every option when adding commands. -->

## Why

`gh api`をそのままAgentに許可すると、読み取りだけに制限しにくく、承認を繰り返すと長時間のtaskが止まります。
`gh-loupe`は開発で使うGitHubの読み取り操作を固定commandにまとめ、更新操作を提供しません。
そのため、command ruleで`gh-loupe`を許可しつつ、Agentが必要な情報だけを段階的に取得できます。

## Install

GitHub CLIで認証してから、installerを実行します。binaryとAgent向けSkillがまとめてインストールされます。

```bash
gh auth login
git clone https://github.com/riii111/gh-loupe.git
cd gh-loupe
./install.sh
```

## Usage

Agentから使う場合は、既定の整形JSONより`--compact`を優先してください。最初から本文や全コメントを取得せず、必要な段階まで進めます。

```bash
PR=https://github.com/OWNER/REPO/pull/123

gh-loupe pr overview --compact "$PR"
gh-loupe pr review-threads --compact "$PR"
gh-loupe pr review-thread --compact "$PR" REVIEW_THREAD_ID
gh-loupe pr checks --failed-only --compact "$PR"
gh-loupe pr checks --failed-only --failed-diagnostics --compact "$PR"
```

主なcommandは次のとおりです。全commandとoptionは`gh-loupe --help`で確認できます。

| Command | 取得内容 |
|---|---|
| `pr overview` | PRの状態、review decision、CI集計 |
| `pr comments` | PR全体へのコメント |
| `pr reviews` | review submission |
| `pr review-threads` | inline review threadの一覧 |
| `pr review-thread` | 指定したinline review thread |
| `pr checks` | checkと失敗診断 |
| `pr for-commit` | commitに関連するPR |
| `issue overview` | Issue本文と件数要約 |
| `issue comments` | IssueのConversation comment |
| `issue relations` | Issueの親子・依存関係 |
| `search issues` / `search prs` | repository内のIssue/PR検索 |

## Token efficiency

実際のPRで`gh api`と比較したtoken数の実測です。`gh-loupe`では29〜98%削減できました。

| Scenario | `gh api` | `gh-loupe` | Reduction |
|---|---:|---:|---:|
| PR overview | 5,032 | 188 | 96% |
| Conversation comments | 11,099 | 6,581 | 41% |
| Review submissions | 30,242 | 21,413 | 29% |
| 未解決review threadの段階取得 | 55,134 | 982 | 98% |

詳細なcommand仕様とJSON schemaは[`docs/reference.md`](docs/reference.md)および各commandの`--help`を参照してください。
