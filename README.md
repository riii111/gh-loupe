# gh-loupe

GitHubの情報を必要な分だけ安全に取得し、AI Agentのトークン消費を抑えるread-only CLIです。

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

## Setup

GitHub CLIで認証後、installerを実行すると、binaryとAgent向けSkillがまとめてインストールされます。

```bash
gh auth login
git clone https://github.com/riii111/gh-loupe.git
cd gh-loupe
./install.sh
```

## Usage

```bash
gh-loupe pr overview https://github.com/OWNER/REPO/pull/123
gh-loupe pr comments https://github.com/OWNER/REPO/pull/123
gh-loupe pr reviews https://github.com/OWNER/REPO/pull/123
gh-loupe pr review-threads https://github.com/OWNER/REPO/pull/123
gh-loupe pr review-thread https://github.com/OWNER/REPO/pull/123 REVIEW_THREAD_ID [REVIEW_THREAD_ID ...]
gh-loupe pr checks https://github.com/OWNER/REPO/pull/123
gh-loupe issue overview https://github.com/OWNER/REPO/issues/123
gh-loupe issue comments https://github.com/OWNER/REPO/issues/123
gh-loupe issue relations https://github.com/OWNER/REPO/issues/123 --limit 20
gh-loupe search issues "keyword" --repo OWNER/REPO --limit 20
gh-loupe search prs "keyword" --repo OWNER/REPO --limit 20
gh-loupe pr for-commit SHA --repo OWNER/REPO --limit 20
```

| Command | 取得内容 |
|---|---|
| `pr overview` | PRの状態、review decision、CI集計、未解決review件数 |
| `pr comments` | PR全体へのコメント |
| `pr reviews` | Approve、Request changesなどのreview submission |
| `pr review-threads` | 未解決inline reviewの一覧 |
| `pr review-thread` | inline review thread一件以上の全コメント |
| `pr checks` | 個別checkと失敗診断 |
| `issue overview` | Issue本文、状態、sub-issue・依存関係の件数要約 |
| `issue comments` | IssueのConversation comment |
| `issue relations` | 親Issue、sub-issue、blockedBy、blockingの一覧 |
| `search issues` | 一つのrepositoryに限定したIssue検索 |
| `search prs` | 一つのrepositoryに限定したPR検索 |
| `pr for-commit` | commit SHAに関連するPRの逆引き |

Issueは概要、comment、関係を別々に取得します。
`issue relations`の`--limit`は各一覧の最大件数で、既定20、1〜100です。
一覧はGitHubの返却順を維持し、`totalCount`と`truncated`で打ち切りを判別できます。
固定schemaの詳細は[`docs/reference.md`](docs/reference.md)を参照してください。

`search issues`と`search prs`は、cwdまたは`--repo OWNER/REPO`で指定した一つのrepositoryだけを検索します。
検索語にrepositoryやIssue/PR種別を指定するqualifierは使えません。
`--limit`は既定20、1〜100で、検索結果には`totalCount`、`truncated`、`incompleteResults`が含まれます。
`pr for-commit`は7〜40文字のhex commit SHAを受け取り、関連PRを`pullRequests`配列で返します。

resolved review threadは既定で除外します。
必要な場合だけ`--include-resolved`を指定します。

`pr review-thread`は既定で`<details>`の折り畳み内容を省略します。
内容が判断に必要な場合だけ`--include-details`を指定します。

`pr review-thread`は1回につき1〜20件のthread IDを受け取り、`data.reviewThreads`へ指定順で返します。
成功時の`data.reviewThreads`は、threadが1件でも必ず配列です。
取得に失敗した場合は、threadを部分的に返さずエラーで終了します。

人間が色付きで確認する場合は`bat`を利用できます。

```bash
gh-loupe pr overview <PR URL> |
  bat --language json --paging never
```

全commandとoptionは`gh-loupe --help`を参照してください。
