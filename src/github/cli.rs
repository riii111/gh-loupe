use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{Exit, Result};

pub(super) fn json<I, S>(args: I, payload: Option<&str>, allow_nonzero_json: bool) -> Result<Value>
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
        write_child_stdin(&mut child, payload).map_err(|error| Exit::message(error.to_string()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| Exit::message(error.to_string()))?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() && !allow_nonzero_json {
        return Err(Exit::child(code, &output.stderr));
    }
    match serde_json::from_slice(&output.stdout) {
        Ok(response) => Ok(response),
        Err(_error) if !output.status.success() => Err(Exit::child(code, &output.stderr)),
        Err(error) => Err(Exit::message(format!(
            "GitHub returned invalid JSON: {error}"
        ))),
    }
}

fn write_child_stdin(child: &mut std::process::Child, payload: &str) -> io::Result<()> {
    let result = child
        .stdin
        .take()
        .expect("stdin is piped when a payload is present")
        .write_all(payload.as_bytes());
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};

    use super::*;

    #[cfg(unix)]
    #[test]
    fn broken_stdin_still_allows_child_status_and_stderr_to_be_collected() {
        let mut child = Command::new("sh")
            .args([
                "-c",
                "exec 0<&-; printf 'ready\\n'; printf 'simulated stdin failure\\n' >&2; exit 29",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn child");

        let mut ready = String::new();
        BufReader::new(child.stdout.take().expect("stdout is piped"))
            .read_line(&mut ready)
            .expect("read synchronization marker");
        assert_eq!(ready, "ready\n");

        write_child_stdin(&mut child, "payload").expect("BrokenPipe is ignored");
        let output = child.wait_with_output().expect("collect child output");
        assert_eq!(output.status.code(), Some(29));
        assert_eq!(output.stderr, b"simulated stdin failure\n");
    }
}
