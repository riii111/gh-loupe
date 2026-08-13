# gh-loupe

GitHubの情報を必要な分だけ安全に取得し、AI Agentのトークン消費を抑えるread-only CLIです。

<!-- Documentation boundary: README explains value and entry points; the Skill explains retrieval choices; command and schema details belong in docs/reference.md and --help. Do not duplicate every option when adding commands. -->

## Why

AI Agentに`gh`でGitHubを操作させる場合、`gh api`をそのまま許可すると、読み取りだけを許可するcommand ruleを書くのが難しくなります。
任意の操作は許可したくない。
しかし、実行のたびに承認を求めれば、頻繁に作業が止まります。

Claude CodeのautoモードやCodexのApprove for meのように、AI Agent自身にcommandの許可を判断させる機能もあります。
これらは便利ですが、Loop Engineeringで長時間taskを任せる場合、GitHub操作のたびにAIへ判断を委ねるより、実行できる操作をあらかじめ読み取りだけに絞る方が安全です。

`gh-loupe`は、そのために、開発中によく使うGitHubの読み取り操作だけを提供します。
command ruleで`gh-loupe`を許可すれば、個々の`gh api`について許可を判断する必要がありません。
Agentを長時間動かしても、`gh-loupe`経由ではGitHubの更新操作を実行できません。
承認待ちと許可判断にかかる時間も減らせます。

## Token efficiency

実際のPRを対象に、`gh-loupe`と`gh api`で取得したJSONを比較しました。
`gh-loupe`では、AI Agentへ渡すトークン数を29〜98%削減できました。

| Scenario | `gh api` | `gh-loupe` | Reduction |
|---|---:|---:|---:|
| PR overview | 5,032 | 188 | 96% |
| Conversation comments | 11,099 | 6,581 | 41% |
| Review submissions | 30,242 | 21,413 | 29% |
| 未解決review threadの段階取得 | 55,134 | 982 | 98% |

手書きで未解決threadへ絞ったGraphQLとの比較でも、段階取得のtoken数は53%少なくなりました。

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

詳細なcommand仕様とJSON schemaは[`docs/reference.md`](docs/reference.md)および各commandの`--help`を参照してください。
