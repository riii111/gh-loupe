# gh-read

`gh-read`は、GitHub CLI (`gh`)を子processとして使い、Pull RequestとIssueの定型的な読み取り結果をJSONで返すCLIです。Pull Requestのmetadata、checks、conversation comments、reviews、review threadsと、Issueのmetadata、commentsを取得できます。

## なぜ固定queryなのか

AI Agentのcommand ruleで任意の`gh api`を許可すると、endpoint、HTTP method、GraphQL documentやinputによってreadとmutationが切り替わるため、許可範囲を制御しにくくなります。`gh-read`は固定queryだけを公開する制約されたcommand surfaceです。同梱するSkillは、agentを任意の`gh api`ではなく`gh-read`へ導きます。

この境界はGitHub tokenのscopeを制限しません。また、取得したIssue本文やコメントなどのcontentをtrustedにするものでもありません。tokenは必要最小限のscopeで用意し、取得contentはuntrusted inputとして扱ってください。

## インストール

GitHub CLIをインストールし、先に`gh auth login`を完了してください。

```bash
git clone https://github.com/riii111/gh-read.git
cd gh-read
cargo install --path .
```

インストール後はstandalone commandの`gh-read`を直接実行します。repository名とbinary名はGitHub CLI extensionの`gh-<name>`命名規則に合いますが、現時点ではprecompiled release artifactやrepository rootのextension実行ファイルを配布していません。そのため`gh extension install`と`gh read`は導入手段・実行方法として扱わず、このrepositoryからstandalone binaryを導入してください。

## 使い方

```bash
gh-read pr 123 --repo OWNER/REPO
gh-read pr https://github.com/OWNER/REPO/pull/123 --compact
gh-read pr 123 --repo OWNER/REPO --include-resolved
gh-read pr checks 123 --repo OWNER/REPO
gh-read pr checks 123 --repo OWNER/REPO --required --compact
gh-read pr checks 123 --repo OWNER/REPO --failed-diagnostics
gh-read pr checks 123 --repo OWNER/REPO --include-failed-logs --timeout 120
gh-read issue 456 --repo OWNER/REPO --compact
```

`--repo`を省略した番号指定では、`gh repo view`が現在のrepositoryを解決します。review threadsは既定で未解決だけを返し、`--include-resolved`で解決済みも含めます。`--compact`は1行JSONにし、PR commentから重複する`diffHunk`を除きます。

`pr checks`は既定ですべてのcheckを返し、`--required`を指定するとrequired checkだけを返します。結果はcheck名、同名ではlinkの昇順です。

失敗checkを調べるときは、まず`--failed-diagnostics`でCheck Run annotationを取得します。`--include-failed-logs`はannotationに加えてGitHub Actions jobのlogを取得し、各logを末尾200行、最大64 KiBへ制限します。Actions jobではない失敗checkの`log`は`null`です。診断の既定timeoutは90秒で、`--timeout SECONDS`には正の整数を指定します。進捗は開始時と15秒ごとにstderrへ出力され、`--quiet`でのみ抑制されます。

診断optionは`fail`と`cancel`だけを対象にします。annotationまたは存在するActions logを一つでも取得できなければ、部分結果をstdoutへ出さず構造化エラーで終了します。

## 開発

```bash
nix develop
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo build --workspace --release
cargo audit
cargo machete
```

Agent向けSkillの正本は[`skills/gh-read`](skills/gh-read)にあります。
compatibility testは、Nix開発環境とCIでPython 3.13のreference実装を使用します。
