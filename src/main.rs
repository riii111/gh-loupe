use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;
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

fn usage(program: &str, resource: Option<Resource>) -> String {
    match resource {
        Some(Resource::Pr) => format!(
            "usage: {program} pr [-h] [--repo REPO] [--include-resolved] [--compact] target"
        ),
        Some(Resource::Issue) => {
            format!("usage: {program} issue [-h] [--repo REPO] [--compact] target")
        }
        None => format!("usage: {program} [-h] {{pr,issue}} ..."),
    }
}

fn argument_error(
    program: &str,
    usage_resource: Option<Resource>,
    error_resource: Option<Resource>,
    message: &str,
) -> Exit {
    let usage = usage(program, usage_resource);
    let error_program = match error_resource {
        Some(Resource::Pr) => format!("{program} pr"),
        Some(Resource::Issue) => format!("{program} issue"),
        None => program.to_owned(),
    };
    Exit {
        message: Some(format!("{usage}\n{error_program}: error: {message}")),
        code: 2,
    }
}

fn is_long_option(value: &str, option: &str) -> bool {
    value.len() > 2 && value.starts_with("--") && option.starts_with(value)
}

fn long_option_value<'a>(value: &'a str, option: &str) -> Option<&'a str> {
    let (name, value) = value.split_once('=')?;
    is_long_option(name, option).then_some(value)
}

fn parse_args() -> Result<Args> {
    let mut values = env::args();
    let program = values.next().unwrap_or_else(|| "gh-read".to_owned());
    let program = Path::new(&program)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gh-read")
        .to_owned();
    let Some(resource_value) = values.next() else {
        return Err(argument_error(
            &program,
            None,
            None,
            "the following arguments are required: resource",
        ));
    };
    if resource_value == "-h" || is_long_option(&resource_value, "--help") {
        print_root_help(&program);
        std::process::exit(0);
    }
    let resource = match resource_value.as_str() {
        "pr" => Resource::Pr,
        "issue" => Resource::Issue,
        other => {
            return Err(argument_error(
                &program,
                None,
                None,
                &format!(
                    "argument resource: invalid choice: '{other}' (choose from 'pr', 'issue')"
                ),
            ));
        }
    };

    let mut target = None;
    let mut repo = None;
    let mut include_resolved = false;
    let mut compact = false;
    let mut positional_only = false;
    let mut unrecognized = Vec::new();
    let mut remaining = values;
    while let Some(value) = remaining.next() {
        if positional_only {
            if target.is_none() {
                target = Some(value);
            } else {
                unrecognized.push(value);
            }
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            option if long_option_value(option, "--repo").is_some() => {
                repo = long_option_value(option, "--repo").map(str::to_owned);
            }
            option if is_long_option(option, "--repo") => {
                let Some(value) = remaining.next() else {
                    return Err(argument_error(
                        &program,
                        Some(resource),
                        Some(resource),
                        "argument --repo: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(argument_error(
                        &program,
                        Some(resource),
                        Some(resource),
                        "argument --repo: expected one argument",
                    ));
                }
                repo = Some(value);
            }
            option if resource == Resource::Pr && is_long_option(option, "--include-resolved") => {
                include_resolved = true;
            }
            option if is_long_option(option, "--compact") => compact = true,
            "-h" => {
                print_help(&program, resource);
                std::process::exit(0);
            }
            option if is_long_option(option, "--help") => {
                print_help(&program, resource);
                std::process::exit(0);
            }
            option if option.starts_with('-') => unrecognized.push(option.to_owned()),
            value if target.is_none() => target = Some(value.to_owned()),
            value => unrecognized.push(value.to_owned()),
        }
    }
    let Some(target) = target else {
        return Err(argument_error(
            &program,
            Some(resource),
            Some(resource),
            "the following arguments are required: target",
        ));
    };
    if !unrecognized.is_empty() {
        return Err(argument_error(
            &program,
            None,
            None,
            &format!("unrecognized arguments: {}", unrecognized.join(" ")),
        ));
    }
    Ok(Args {
        resource,
        target,
        repo,
        include_resolved,
        compact,
    })
}

fn print_root_help(program: &str) {
    let text = format!(
        "{}\n\nRead fixed GitHub PR and Issue metadata without mutations.\n\npositional arguments:\n  {{pr,issue}}\n    pr        read pull request metadata and review data\n    issue     read issue metadata and comments\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program, None)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_help(program: &str, resource: Resource) {
    let text = match resource {
        Resource::Pr => format!(
            "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           omit repeated diff hunks and emit compact JSON\n",
            usage(program, Some(resource))
        ),
        Resource::Issue => format!(
            "{}\n\npositional arguments:\n  target       Issue number or GitHub issue URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
            usage(program, Some(resource))
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

fn gh_json<I, S>(args: I, payload: Option<&str>, allow_nonzero_json: bool) -> Result<Value>
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
        child
            .stdin
            .take()
            .expect("stdin is piped when a payload is present")
            .write_all(payload.as_bytes())
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

fn graphql(query: &str, variables: &str) -> Result<Value> {
    let query = serde_json::to_string(query)
        .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
    let payload = format!(r#"{{"query":{query},"variables":{variables}}}"#);
    let response = gh_json(["api", "graphql", "--input", "-"], Some(&payload), false)?;
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

fn positive_number(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = value.trim_start_matches('0');
    (!value.is_empty()).then(|| value.to_owned())
}

fn resolve_target(
    target: &str,
    repo: Option<String>,
    resource: Resource,
) -> Result<(String, String)> {
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
        let number = positive_number(number);
        if let Some(number) = number {
            return Ok((url_repo.to_owned(), number));
        }
    }

    let number = positive_number(target);
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

fn fetch_threads(repo: &str, number: &str, include_resolved: bool) -> Result<(Value, Vec<Value>)> {
    let (owner, name) = repo.split_once('/').expect("repository is validated");
    let mut cursor = Value::Null;
    let mut threads = Vec::new();

    let pull_request = loop {
        let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
        let name = serde_json::to_string(name).expect("serializing a string cannot fail");
        let cursor_json = serde_json::to_string(&cursor)
            .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
        let variables = format!(
            r#"{{"owner":{owner},"name":{name},"number":{number},"cursor":{cursor_json}}}"#
        );
        let data = graphql(THREADS_QUERY, &variables)?;
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
            let variables =
                serde_json::to_string(&json!({"id": id, "cursor": cursor})).map_err(|error| {
                    Exit::message(format!("failed to encode GitHub request: {error}"))
                })?;
            let data = graphql(COMMENTS_QUERY, &variables)?;
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

fn fetch_checks(repo: &str, number: &str) -> Result<Value> {
    let checks = gh_json(
        [
            "pr",
            "checks",
            number,
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

fn fetch_issue(repo: &str, number: &str) -> Result<Value> {
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
            let (pull_request, mut threads) = fetch_threads(&repo, &number, args.include_resolved)?;
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
            result.insert("checks".to_owned(), fetch_checks(&repo, &number)?);
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
            result.insert("issue".to_owned(), fetch_issue(&repo, &number)?);
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
