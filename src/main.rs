use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

const THREADS_QUERY: &str = r"
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
";

const COMMENTS_QUERY: &str = r"
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
";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Pr,
    Issue,
}

struct Args {
    resource: Resource,
    target: String,
    repo: Option<String>,
    include_resolved: bool,
    compact: bool,
}

struct Exit {
    message: Option<String>,
    code: i32,
}

type Result<T> = std::result::Result<T, Exit>;

impl Exit {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            code: 1,
        }
    }

    const fn code(code: i32) -> Self {
        Self {
            message: None,
            code,
        }
    }
}

fn argument_error(program: &str, resource: Option<Resource>, message: &str) -> Exit {
    let usage = match resource {
        Some(Resource::Pr) => format!(
            "usage: {program} pr [-h] [--repo REPO] [--include-resolved] [--compact] target"
        ),
        Some(Resource::Issue) => {
            format!("usage: {program} issue [-h] [--repo REPO] [--compact] target")
        }
        None => format!("usage: {program} [-h] {{pr,issue}} ..."),
    };
    Exit {
        message: Some(format!("{usage}\n{program}: error: {message}")),
        code: 2,
    }
}

fn parse_args() -> Result<Args> {
    let mut values = env::args();
    let program = values.next().unwrap_or_else(|| "gh-read".to_owned());
    let Some(resource_value) = values.next() else {
        return Err(argument_error(
            &program,
            None,
            "the following arguments are required: resource",
        ));
    };
    let resource = match resource_value.as_str() {
        "pr" => Resource::Pr,
        "issue" => Resource::Issue,
        other => {
            return Err(argument_error(
                &program,
                None,
                &format!("argument resource: invalid choice: '{other}' (choose from pr, issue)"),
            ));
        }
    };

    let mut target = None;
    let mut repo = None;
    let mut include_resolved = false;
    let mut compact = false;
    let mut remaining = values;
    while let Some(value) = remaining.next() {
        match value.as_str() {
            "--repo" => {
                let Some(value) = remaining.next() else {
                    return Err(argument_error(
                        &program,
                        Some(resource),
                        "argument --repo: expected one argument",
                    ));
                };
                repo = Some(value);
            }
            option if option.starts_with("--repo=") => {
                repo = Some(option["--repo=".len()..].to_owned());
            }
            "--include-resolved" if resource == Resource::Pr => include_resolved = true,
            "--compact" => compact = true,
            "-h" | "--help" => {
                print_help(&program, resource);
                std::process::exit(0);
            }
            option if option.starts_with('-') => {
                return Err(argument_error(
                    &program,
                    Some(resource),
                    &format!("unrecognized arguments: {option}"),
                ));
            }
            value if target.is_none() => target = Some(value.to_owned()),
            value => {
                return Err(argument_error(
                    &program,
                    Some(resource),
                    &format!("unrecognized arguments: {value}"),
                ));
            }
        }
    }
    let Some(target) = target else {
        return Err(argument_error(
            &program,
            Some(resource),
            "the following arguments are required: target",
        ));
    };
    Ok(Args {
        resource,
        target,
        repo,
        include_resolved,
        compact,
    })
}

fn print_help(program: &str, resource: Resource) {
    let text = match resource {
        Resource::Pr => format!(
            "usage: {program} pr [-h] [--repo REPO] [--include-resolved] [--compact] target\n\nread pull request metadata and review data\n"
        ),
        Resource::Issue => format!(
            "usage: {program} issue [-h] [--repo REPO] [--compact] target\n\nread issue metadata and comments\n"
        ),
    };
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn stderr_line(message: &str) {
    writeln!(io::stderr(), "{message}").expect("write error");
}

fn stdout_line(message: &str) {
    writeln!(io::stdout(), "{message}").expect("write output");
}

fn gh_json<I, S>(args: I, payload: Option<&Value>, allow_nonzero_json: bool) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("gh");
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if payload.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|error| Exit::message(error.to_string()))?;
    if let Some(payload) = payload {
        let encoded = serde_json::to_vec(payload)
            .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
        child
            .stdin
            .take()
            .expect("stdin is piped when a payload is present")
            .write_all(&encoded)
            .map_err(|error| Exit::message(error.to_string()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| Exit::message(error.to_string()))?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() && !allow_nonzero_json {
        stderr_line(String::from_utf8_lossy(&output.stderr).trim_end());
        return Err(Exit::code(code));
    }
    match serde_json::from_slice(&output.stdout) {
        Ok(response) => Ok(response),
        Err(_error) if !output.status.success() => {
            stderr_line(String::from_utf8_lossy(&output.stderr).trim_end());
            Err(Exit::code(code))
        }
        Err(error) => Err(Exit::message(format!(
            "GitHub returned invalid JSON: {error}"
        ))),
    }
}

fn python_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        Value::Array(values) => {
            let values = values
                .iter()
                .map(python_json)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{values}]")
        }
        Value::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(key).expect("serializing a string cannot fail"),
                        python_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{values}}}")
        }
    }
}

fn graphql(query: &str, variables: Value) -> Result<Value> {
    let response = gh_json(
        ["api", "graphql", "--input", "-"],
        Some(&json!({"query": query, "variables": variables})),
        false,
    )?;
    if let Some(errors) = response.get("errors") {
        stderr_line(&python_json(errors));
        return Err(Exit::code(1));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| Exit::message("GitHub returned a GraphQL response without data"))
}

fn rest_pages(endpoint: &str) -> Result<Vec<Value>> {
    let pages = gh_json(
        ["api", "--method", "GET", "--paginate", "--slurp", endpoint],
        None,
        false,
    )?;
    let pages = pages
        .as_array()
        .ok_or_else(|| Exit::message("GitHub returned an invalid paginated response"))?;
    if pages.iter().any(|page| !page.is_array()) {
        return Err(Exit::message(
            "GitHub returned an invalid paginated response",
        ));
    }
    let items = pages
        .iter()
        .flat_map(|page| page.as_array().expect("validated above").iter().cloned())
        .collect::<Vec<_>>();
    if items.iter().any(|item| !item.is_object()) {
        return Err(Exit::message("GitHub returned invalid paginated items"));
    }
    Ok(items)
}

fn is_repo(value: &str) -> bool {
    let mut segments = value.split('/');
    let Some(owner) = segments.next() else {
        return false;
    };
    let Some(name) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && [owner, name].into_iter().all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        })
}

fn resolve_repo(repo: Option<String>) -> Result<String> {
    let repo = if let Some(repo) = repo {
        repo
    } else {
        let response = gh_json(["repo", "view", "--json", "nameWithOwner"], None, false)?;
        response
            .get("nameWithOwner")
            .and_then(Value::as_str)
            .ok_or_else(|| Exit::message("GitHub returned an invalid repository response"))?
            .to_owned()
    };
    if !is_repo(&repo) {
        return Err(Exit::message("--repo must use OWNER/REPO format"));
    }
    Ok(repo)
}

fn parse_url(target: &str, resource: Resource) -> Option<(&str, &str)> {
    let path = target.strip_prefix("https://github.com/")?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    let kind = segments.next()?;
    let number = segments.next()?;
    if segments.next().is_some()
        || kind
            != match resource {
                Resource::Pr => "pull",
                Resource::Issue => "issues",
            }
    {
        return None;
    }
    let repo_length = owner.len() + 1 + name.len();
    Some((&path[..repo_length], number))
}

fn resolve_target(target: &str, repo: Option<String>, resource: Resource) -> Result<(String, u64)> {
    if let Some((url_repo, number)) = parse_url(target, resource) {
        if !is_repo(url_repo) {
            let name = match resource {
                Resource::Pr => "pr",
                Resource::Issue => "issue",
            };
            return Err(Exit::message(format!(
                "{name} URL must contain a valid OWNER/REPO"
            )));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            return Err(Exit::message("--repo conflicts with the pull request URL"));
        }
        let number = number.parse::<u64>().ok().filter(|number| *number > 0);
        if let Some(number) = number {
            return Ok((url_repo.to_owned(), number));
        }
    }

    let number = target
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| target.parse::<u64>().ok())
        .flatten()
        .filter(|number| *number > 0);
    let Some(number) = number else {
        let name = match resource {
            Resource::Pr => "pr",
            Resource::Issue => "issue",
        };
        return Err(Exit::message(format!(
            "{name} must be a positive number or GitHub {name} URL"
        )));
    };
    Ok((resolve_repo(repo)?, number))
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value
            .get(*key)
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))
    })
}

fn fetch_threads(repo: &str, number: u64, include_resolved: bool) -> Result<(Value, Vec<Value>)> {
    let (owner, name) = repo.split_once('/').expect("repository is validated");
    let mut cursor = Value::Null;
    let mut threads = Vec::new();

    let pull_request = loop {
        let data = graphql(
            THREADS_QUERY,
            json!({"owner": owner, "name": name, "number": number, "cursor": cursor}),
        )?;
        let current = value_at(&data, &["repository", "pullRequest"])?;
        if current.is_null() {
            return Err(Exit::message(format!(
                "pull request not found: {repo}#{number}"
            )));
        }
        let mut current = current.clone();
        let connection = current
            .as_object_mut()
            .and_then(|current| current.shift_remove("reviewThreads"))
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
        threads.extend(
            value_at(&connection, &["nodes"])?
                .as_array()
                .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
                .iter()
                .cloned(),
        );
        if !value_at(&connection, &["pageInfo", "hasNextPage"])?
            .as_bool()
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
        {
            break current;
        }
        cursor = value_at(&connection, &["pageInfo", "endCursor"])?.clone();
    };

    if !include_resolved {
        threads.retain(|thread| thread.get("isResolved") != Some(&Value::Bool(true)));
    }

    for thread in &mut threads {
        let id = thread
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
            .to_owned();
        let comments = thread
            .as_object_mut()
            .and_then(|thread| thread.get_mut("comments"))
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
        let mut cursor = value_at(comments, &["pageInfo", "endCursor"])?.clone();
        while value_at(comments, &["pageInfo", "hasNextPage"])?
            .as_bool()
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
        {
            let data = graphql(COMMENTS_QUERY, json!({"id": id, "cursor": cursor}))?;
            let page = value_at(&data, &["node", "comments"])?;
            let nodes = value_at(page, &["nodes"])?
                .as_array()
                .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
                .clone();
            comments
                .get_mut("nodes")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
                .extend(nodes);
            comments
                .as_object_mut()
                .expect("comments is an object")
                .insert(
                    "pageInfo".to_owned(),
                    value_at(page, &["pageInfo"])?.clone(),
                );
            cursor = value_at(page, &["pageInfo", "endCursor"])?.clone();
        }
        let nodes = comments
            .as_object_mut()
            .and_then(|comments| comments.shift_remove("nodes"))
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
        thread
            .as_object_mut()
            .expect("thread is an object")
            .insert("comments".to_owned(), nodes);
    }

    Ok((pull_request, threads))
}

fn fetch_checks(repo: &str, number: u64) -> Result<Value> {
    let checks = gh_json(
        [
            "pr",
            "checks",
            &number.to_string(),
            "--repo",
            repo,
            "--json",
            "name,state,bucket,link,workflow,startedAt,completedAt",
        ],
        None,
        true,
    )?;
    if !checks.is_array() {
        return Err(Exit::message("GitHub returned an invalid checks response"));
    }
    Ok(checks)
}

fn fetch_issue(repo: &str, number: u64) -> Result<Value> {
    let issue = gh_json(
        ["api", &format!("repos/{repo}/issues/{number}")],
        None,
        false,
    )?;
    if !issue.is_object() {
        return Err(Exit::message("GitHub returned an invalid issue response"));
    }
    Ok(issue)
}

fn run() -> Result<()> {
    let args = parse_args()?;
    let (repo, number) = resolve_target(&args.target, args.repo, args.resource)?;
    let mut result = Map::new();
    match args.resource {
        Resource::Pr => {
            let (pull_request, mut threads) = fetch_threads(&repo, number, args.include_resolved)?;
            if args.compact {
                for thread in &mut threads {
                    if let Some(comments) = thread.get_mut("comments").and_then(Value::as_array_mut)
                    {
                        for comment in comments {
                            if let Some(comment) = comment.as_object_mut() {
                                comment.shift_remove("diffHunk");
                            }
                        }
                    }
                }
            }
            result.insert("pullRequest".to_owned(), pull_request);
            result.insert("checks".to_owned(), fetch_checks(&repo, number)?);
            result.insert(
                "conversationComments".to_owned(),
                Value::Array(rest_pages(&format!(
                    "repos/{repo}/issues/{number}/comments?per_page=100"
                ))?),
            );
            result.insert(
                "reviews".to_owned(),
                Value::Array(rest_pages(&format!(
                    "repos/{repo}/pulls/{number}/reviews?per_page=100"
                ))?),
            );
            result.insert("reviewThreads".to_owned(), Value::Array(threads));
            result.insert(
                "includesResolvedThreads".to_owned(),
                Value::Bool(args.include_resolved),
            );
        }
        Resource::Issue => {
            result.insert("issue".to_owned(), fetch_issue(&repo, number)?);
            result.insert(
                "comments".to_owned(),
                Value::Array(rest_pages(&format!(
                    "repos/{repo}/issues/{number}/comments?per_page=100"
                ))?),
            );
        }
    }
    let result = Value::Object(result);
    let output = if args.compact {
        serde_json::to_string(&result)
    } else {
        serde_json::to_string_pretty(&result)
    }
    .map_err(|error| Exit::message(error.to_string()))?;
    stdout_line(&output);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        if let Some(message) = error.message {
            stderr_line(&message);
        }
        std::process::exit(error.code);
    }
}
