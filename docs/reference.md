# Command reference

All commands are read-only. `TARGET` is an Issue or pull request number, or its GitHub URL. A number uses the repository inferred from the current directory unless `--repo OWNER/REPO` is supplied.

## Issue

Issue commands require an explicit subcommand:

```shell
gh-loupe issue overview TARGET
gh-loupe issue comments TARGET
gh-loupe issue relations TARGET [--limit N]
```

`overview` returns `data.repository` and `data.issue`. The Issue object contains the fixed metadata schema, including `body`, `state`, `stateReason`, `subIssues`, and `dependencies`. It does not retrieve or return comments.

`comments` returns `data.repository` and `data.comments`. Each comment has only `id`, `url`, `author`, `body`, `createdAt`, and `updatedAt`. All pages are retrieved and comments are ordered by `createdAt`, then by `id` for equal timestamps.

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

## Search

Search commands are limited to one repository. `--repo OWNER/REPO` selects it explicitly; when omitted, the repository is inferred from the current directory and the command fails if inference is unavailable.

```shell
gh-loupe search issues QUERY [--repo OWNER/REPO] [--limit N]
gh-loupe search prs QUERY [--repo OWNER/REPO] [--limit N]
gh-loupe pr for-commit SHA [--repo OWNER/REPO] [--limit N]
```

`search issues` and `search prs` use the fixed GitHub Search issues REST GET endpoint. The command adds `repo:OWNER/REPO` and `is:issue` or `is:pr`; `QUERY` cannot add or override `repo:`, `org:`, `user:`, `is:issue`, or `is:pr`. No global, organization-wide, or user-wide search is exposed.

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
