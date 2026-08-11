#!/usr/bin/env python3

import argparse
import json
import re
import subprocess
import sys
from typing import Any


PR_URL = re.compile(
    r"https://github\.com/(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/(?P<number>[1-9][0-9]*)/?"
)
ISSUE_URL = re.compile(
    r"https://github\.com/(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/issues/(?P<number>[1-9][0-9]*)/?"
)
REPO = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")

THREADS_QUERY = """
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      url
      title
      state
      isDraft
      headRefOid
      baseRefOid
      reviewThreads(first: 100, after: $cursor) {
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          originalLine
          startLine
          diffSide
          resolvedBy { login }
          comments(first: 100) {
            nodes {
              id
              databaseId
              url
              body
              author { login }
              createdAt
              updatedAt
              path
              line
              originalLine
              diffHunk
              replyTo { id }
            }
            pageInfo { hasNextPage endCursor }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
"""

COMMENTS_QUERY = """
query($id: ID!, $cursor: String) {
  node(id: $id) {
    ... on PullRequestReviewThread {
      comments(first: 100, after: $cursor) {
        nodes {
          id
          databaseId
          url
          body
          author { login }
          createdAt
          updatedAt
          path
          line
          originalLine
          diffHunk
          replyTo { id }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
"""


def gh_json(
    args: list[str],
    payload: dict[str, Any] | None = None,
    *,
    allow_nonzero_json: bool = False,
) -> Any:
    result = subprocess.run(
        ["gh", *args],
        check=False,
        input=None if payload is None else json.dumps(payload),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0 and not allow_nonzero_json:
        print(result.stderr.rstrip(), file=sys.stderr)
        raise SystemExit(result.returncode or 1)
    try:
        response = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        if result.returncode != 0:
            print(result.stderr.rstrip(), file=sys.stderr)
            raise SystemExit(result.returncode or 1)
        raise SystemExit(f"GitHub returned invalid JSON: {error}") from error
    return response


def graphql(query: str, variables: dict[str, Any]) -> dict[str, Any]:
    response = gh_json(
        ["api", "graphql", "--input", "-"],
        {"query": query, "variables": variables},
    )
    if response.get("errors"):
        print(json.dumps(response["errors"], ensure_ascii=False), file=sys.stderr)
        raise SystemExit(1)
    return response["data"]


def rest_pages(endpoint: str) -> list[dict[str, Any]]:
    pages = gh_json(["api", "--method", "GET", "--paginate", "--slurp", endpoint])
    if not isinstance(pages, list) or any(not isinstance(page, list) for page in pages):
        raise SystemExit("GitHub returned an invalid paginated response")
    items = [item for page in pages for item in page]
    if any(not isinstance(item, dict) for item in items):
        raise SystemExit("GitHub returned invalid paginated items")
    return items


def is_repo(value: str) -> bool:
    return REPO.fullmatch(value) is not None and all(
        segment not in {".", ".."} for segment in value.split("/")
    )


def resolve_repo(repo: str | None) -> str:
    if repo is None:
        repo = gh_json(["repo", "view", "--json", "nameWithOwner"])["nameWithOwner"]
    if not is_repo(repo):
        raise SystemExit("--repo must use OWNER/REPO format")
    return repo


def resolve_target(
    target: str, repo: str | None, resource: str
) -> tuple[str, int]:
    url_pattern = PR_URL if resource == "pr" else ISSUE_URL
    url_match = url_pattern.fullmatch(target)
    if url_match:
        url_repo = url_match.group("repo")
        if not is_repo(url_repo):
            raise SystemExit(f"{resource} URL must contain a valid OWNER/REPO")
        if repo is not None and repo.lower() != url_repo.lower():
            raise SystemExit("--repo conflicts with the pull request URL")
        return url_repo, int(url_match.group("number"))

    if re.fullmatch(r"[0-9]+", target) is None or int(target) < 1:
        raise SystemExit(
            f"{resource} must be a positive number or GitHub {resource} URL"
        )
    return resolve_repo(repo), int(target)


def fetch_threads(
    repo: str, number: int, *, include_resolved: bool
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    owner, name = repo.split("/", 1)
    cursor = None
    pull_request = None
    threads: list[dict[str, Any]] = []

    while True:
        data = graphql(
            THREADS_QUERY,
            {"owner": owner, "name": name, "number": number, "cursor": cursor},
        )
        pull_request = data["repository"]["pullRequest"]
        if pull_request is None:
            raise SystemExit(f"pull request not found: {repo}#{number}")
        connection = pull_request.pop("reviewThreads")
        threads.extend(connection["nodes"])
        if not connection["pageInfo"]["hasNextPage"]:
            break
        cursor = connection["pageInfo"]["endCursor"]

    if not include_resolved:
        threads = [thread for thread in threads if not thread["isResolved"]]

    for thread in threads:
        comments = thread["comments"]
        cursor = comments["pageInfo"]["endCursor"]
        while comments["pageInfo"]["hasNextPage"]:
            data = graphql(COMMENTS_QUERY, {"id": thread["id"], "cursor": cursor})
            page = data["node"]["comments"]
            comments["nodes"].extend(page["nodes"])
            comments["pageInfo"] = page["pageInfo"]
            cursor = page["pageInfo"]["endCursor"]
        thread["comments"] = comments["nodes"]

    assert pull_request is not None
    return pull_request, threads


def fetch_checks(repo: str, number: int) -> list[dict[str, Any]]:
    checks = gh_json(
        [
            "pr",
            "checks",
            str(number),
            "--repo",
            repo,
            "--json",
            "name,state,bucket,link,workflow,startedAt,completedAt",
        ],
        allow_nonzero_json=True,
    )
    if not isinstance(checks, list):
        raise SystemExit("GitHub returned an invalid checks response")
    return checks


def fetch_issue(repo: str, number: int) -> dict[str, Any]:
    issue = gh_json(["api", f"repos/{repo}/issues/{number}"])
    if not isinstance(issue, dict):
        raise SystemExit("GitHub returned an invalid issue response")
    return issue


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Read fixed GitHub PR and Issue metadata without mutations.",
        allow_abbrev=False,
    )
    parser.add_argument(
        "--version",
        action="store_true",
        help="show program's version and exit",
    )
    subparsers = parser.add_subparsers(dest="resource", required=True)
    pr = subparsers.add_parser("pr", help="read pull request metadata and review data")
    pr.allow_abbrev = False
    pr.add_argument("target", help="PR number or GitHub pull request URL")
    pr.add_argument("--repo", help="OWNER/REPO; inferred from cwd when omitted")
    pr.add_argument(
        "--include-resolved",
        action="store_true",
        help="include resolved review threads",
    )
    pr.add_argument(
        "--compact",
        action="store_true",
        help="omit repeated diff hunks and emit compact JSON",
    )
    issue = subparsers.add_parser("issue", help="read issue metadata and comments")
    issue.allow_abbrev = False
    issue.add_argument("target", help="Issue number or GitHub issue URL")
    issue.add_argument("--repo", help="OWNER/REPO; inferred from cwd when omitted")
    issue.add_argument("--compact", action="store_true", help="emit one-line JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo, number = resolve_target(args.target, args.repo, args.resource)
    if args.resource == "pr":
        pull_request, threads = fetch_threads(
            repo, number, include_resolved=args.include_resolved
        )
        if args.compact:
            for thread in threads:
                for comment in thread["comments"]:
                    comment.pop("diffHunk", None)
        result = {
            "pullRequest": pull_request,
            "checks": fetch_checks(repo, number),
            "conversationComments": rest_pages(
                f"repos/{repo}/issues/{number}/comments?per_page=100"
            ),
            "reviews": rest_pages(f"repos/{repo}/pulls/{number}/reviews?per_page=100"),
            "reviewThreads": threads,
            "includesResolvedThreads": args.include_resolved,
        }
    else:
        result = {
            "issue": fetch_issue(repo, number),
            "comments": rest_pages(f"repos/{repo}/issues/{number}/comments?per_page=100"),
        }
    json.dump(
        result,
        sys.stdout,
        ensure_ascii=False,
        indent=None if args.compact else 2,
        separators=(",", ":") if args.compact else None,
    )
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
