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
