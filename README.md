# gh-read

GitHubの情報を必要な分だけ安全に取得し、AI Agentのトークン消費を抑えるread-only CLIです。

## Why

AI Agentに`gh`でGitHubを操作させる場合、`gh api`をそのまま許可すると、読み取りだけを許可するcommand ruleを書くのが難しくなります。
任意の操作は許可したくない。
しかし、実行のたびに承認を求めれば、頻繁に作業が止まります。

Claude CodeのautoモードやCodexのApprove for meのように、AI Agent自身にcommandの許可を判断させる機能もあります。
これらは便利ですが、Loop Engineeringで長時間taskを任せる場合、GitHub操作のたびにAIへ判断を委ねるより、実行できる操作をあらかじめ読み取りだけに絞る方が安全です。

`gh-read`は、そのために、開発中によく使うGitHubの読み取り操作だけを提供します。
command ruleで`gh-read`を許可すれば、個々の`gh api`について許可を判断する必要がありません。
Agentを長時間動かしても、`gh-read`経由ではGitHubの更新操作を実行できません。
承認待ちと許可判断にかかる時間も減らせます。

## Setup

GitHub CLIで認証後、installerを実行します。

```bash
gh auth login
git clone https://github.com/riii111/gh-read.git
cd gh-read
./install.sh
```

binaryとAgent向けSkillがまとめてインストールされます。

## Usage

```bash
gh-read pr overview https://github.com/OWNER/REPO/pull/123
gh-read pr comments https://github.com/OWNER/REPO/pull/123
gh-read pr reviews https://github.com/OWNER/REPO/pull/123
gh-read pr review-threads https://github.com/OWNER/REPO/pull/123
gh-read pr checks https://github.com/OWNER/REPO/pull/123
```

| Command | 取得内容 |
|---|---|
| `pr overview` | PRの状態、review decision、CI集計、未解決review件数 |
| `pr comments` | PR全体へのコメント |
| `pr reviews` | Approve、Request changesなどのreview submission |
| `pr review-threads` | 未解決inline reviewの一覧 |
| `pr review-thread` | inline review一件の全コメント |
| `pr checks` | 個別checkと失敗診断 |

resolved review threadは既定で除外します。
必要な場合だけ`--include-resolved`を指定します。

人間が色付きで確認する場合は`bat`を利用できます。

```bash
gh-read pr overview <PR URL> |
  bat --language json --paging never
```

全commandとoptionは`gh-read --help`を参照してください。
