# Command reference

All commands are read-only. `TARGET` is an Issue or pull request number, or its GitHub URL. A number uses the repository inferred from the current directory unless `--repo OWNER/REPO` is supplied.

## Issue

Issue commands require an explicit subcommand:

```shell
gh-loupe issue overview TARGET
gh-loupe issue comments TARGET [--include-details] [--limit N] [--since TIMESTAMP]
gh-loupe issue relations TARGET [--limit N]
```

`overview` returns `data.repository` and `data.issue`. The Issue object contains the fixed metadata schema, including `body`, `state`, `stateReason`, `subIssues`, and `dependencies`. It does not retrieve or return comments.

`comments` returns `data.repository` and `data.comments`. Each comment has only `id`, `url`, `author`, `body`, `createdAt`, `updatedAt`, and `detailsOmitted`. All pages are retrieved and comments are ordered by `createdAt`, then by `id` for equal timestamps.

`--limit` accepts 1 through 100 and returns the latest comments in the same chronological order. `--since TIMESTAMP` selects comments whose `updatedAt` is later than the timestamp, including comments created earlier and edited afterward. Either option adds `totalCount` for the comments remaining after `--since` and before `--limit`, and `truncated`, which is true only when `--limit` omits comments. Without either option, the existing response shape is preserved.

By default, closed `<details>` blocks in `body` are omitted using the same Markdown rules as pull request reviews. Human text, `<details open>`, incomplete or malformed tags, and tags inside code spans, fenced code, or HTML comments are preserved. `detailsOmitted` is true only when that comment body was changed. `--include-details` returns the original body and sets `detailsOmitted` to false for every comment.

`relations` returns `data.repository`, `data.parent`, `data.subIssues`, `data.blockedBy`, and `data.blocking`.

The parent is a relation summary or `null`. Each relation summary has `repository`, `number`, `title`, `url`, `state`, `stateReason`, and `assignees`. `repository` is always included because a relation may target another repository.

Each list has this shape:

```json
{
  "items": [],
  "totalCount": 0,
  "truncated": false
}
```

`--limit` defaults to 20 and accepts 1 through 100. It applies to the three lists, not to `parent`. The list order is the order returned by the corresponding GitHub GraphQL connection; `gh-loupe` does not sort the list. `truncated` is true when `totalCount` exceeds the requested limit.

No Issue or comment body is included in `relations`. A malformed or partial relation response, including a GraphQL error, fails the command without emitting partial JSON.

## Pull request checks

```shell
gh-loupe pr checks TARGET
gh-loupe pr checks TARGET --failed-only [--required] [--failed-diagnostics] [--include-failed-logs]
```

The default output has `data.checks` with the existing check metadata. `--failed-diagnostics` does not change that output to failed-only. With `--failed-only`, the output has `data.summary` and `data.checks`; `summary` contains `total`, `passed`, `pending`, and `failed` for all retrieved checks, while `checks` contains only `fail` and `cancel` buckets. `skipping` counts as `passed` and `cancel` counts as `failed`. `--required` applies to both the summary and returned checks. Zero failures is a successful result with `summary.failed` set to `0` and an empty `checks` array.

## Pull request overview

```shell
gh-loupe pr overview TARGET [--include-body] [--include-details]
```

The default output keeps the existing `data.pullRequest` schema and does not retrieve or return the pull request body. `--include-body` adds `body` and `detailsOmitted` to `data.pullRequest`. Closed `<details>` blocks in the body are omitted by default using the shared Markdown rules; `detailsOmitted` is true only when the body was changed. `--include-details` returns the original body and is accepted only together with `--include-body`.

## Pull request comments

```shell
gh-loupe pr comments TARGET [--include-details] [--limit N] [--since TIMESTAMP]
```

`pr comments` returns `data.comments`. Each comment has only `id`, `url`, `author`, `body`, `createdAt`, `updatedAt`, and `detailsOmitted`. All pages are retrieved and comments are ordered by `createdAt`, then by `id` for equal timestamps.

`--limit` accepts 1 through 100 and returns the latest comments in the same chronological order. `--since TIMESTAMP` selects comments whose `updatedAt` is later than the timestamp, including comments created earlier and edited afterward. Either option adds `totalCount` for the comments remaining after `--since` and before `--limit`, and `truncated`, which is true only when `--limit` omits comments. Without either option, the existing response shape is preserved.

By default, closed `<details>` blocks in `body` are omitted using the same Markdown rules as `issue comments`, `pr reviews`, and `pr review-thread`. Human text, `<details open>`, incomplete or malformed tags, and tags inside code spans, fenced code, or HTML comments are preserved. `detailsOmitted` is true only when that comment body was changed. `--include-details` returns the original body and sets `detailsOmitted` to false for every comment.

## Pull request files

```shell
gh-loupe pr files TARGET [--limit N]
```

`pr files` returns at most `N` entries in `data.files`. Each entry contains only `path`, `status`, `additions`, and `deletions`; patches, diff hunks, and file contents are not retrieved. `--limit` defaults to 20 and accepts 1 through 100.

`data.summary` describes the whole pull request, not only the returned entries. It contains `total`, `additions`, and `deletions`. `data.totalCount` is the whole pull request file count, and `data.truncated` is true when the returned list is incomplete, including when GitHub cannot expose every changed file.

## Pull request reviews

```shell
gh-loupe pr reviews TARGET [--include-details] [--limit N]
```

`pr reviews` retrieves every REST review page and preserves chronological order by `submittedAt`, then by `id` for equal timestamps. Each review contains only `id`, `author`, `state`, `body`, `submittedAt`, `commitOid`, and `detailsOmitted`.

`--limit` accepts 1 through 100 and returns the latest reviews in the same chronological order. The limited response also contains `totalCount` for all review submissions and `truncated`, which is true only when a review submission was not returned. Without `--limit`, all reviews are returned with the existing response shape. Reviews with a null `submittedAt` remain in the result and retain their existing ordering.

By default, closed `<details>` blocks in `body` are omitted using the same Markdown rules as `pr review-thread`. Human text, `<details open>`, incomplete or malformed tags, and tags inside code spans, fenced code, or HTML comments are preserved. `detailsOmitted` is true only when that review body was changed. `--include-details` returns the original body and sets `detailsOmitted` to false for every review.

## Pull request review threads

```shell
gh-loupe pr review-threads TARGET [--include-resolved]
gh-loupe pr review-thread TARGET REVIEW_THREAD_ID [REVIEW_THREAD_ID ...]
```

`review-threads` returns unresolved threads by default. `--include-resolved` includes resolved threads as well.

`review-thread` accepts 1 through 20 thread IDs, returns them in input order under `data.reviewThreads`, and emits no partial JSON when any requested thread fails. Closed `<details>` blocks in comments are omitted by default; `--include-details` preserves them, and `--include-diff-hunk` adds `diffHunk` to every comment.

## Search

Search commands are limited to one repository. `--repo OWNER/REPO` selects it explicitly; when omitted, the repository is inferred from the current directory and the command fails if inference is unavailable.

```shell
gh-loupe search issues QUERY [--repo OWNER/REPO] [--limit N]
gh-loupe search prs QUERY [--repo OWNER/REPO] [--limit N]
gh-loupe pr for-commit SHA [--repo OWNER/REPO] [--limit N]
```

`search issues` and `search prs` use the fixed GitHub Search issues REST GET endpoint. The command adds `repo:OWNER/REPO` and `is:issue` or `is:pr`; `QUERY` cannot add or override `repo:`, `org:`, `user:`, `is:issue`, `is:pr`, `type:issue`, or `type:pr`. No global, organization-wide, or user-wide search is exposed.

`--limit` defaults to 20 and accepts 1 through 100. Search output has this shape:

```json
{
  "repository": "OWNER/REPO",
  "issues": [],
  "totalCount": 0,
  "truncated": false,
  "incompleteResults": false
}
```

Issue items contain only `number`, `title`, `url`, `state`, `updatedAt`, and nullable `stateReason`. PR items contain only `number`, `title`, `url`, `state`, `updatedAt`, and `isDraft`. The response marker is checked strictly: an issue search item with a `pull_request` marker, or a PR search item without one, is invalid.

`pr for-commit` uses the fixed commit-to-pull-requests REST GET endpoint. `SHA` must be 7 through 40 hexadecimal characters; branch and ref names are rejected before any GitHub request. Its output has `repository`, a `pullRequests` array using the PR item summary above, and `truncated`. An empty array means that no related PR exists. Commit endpoint PR objects are validated without requiring the Search API's `pull_request` marker.
