use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, Target};

use super::actions_log::{LOG_BYTE_LIMIT, LOG_LINE_LIMIT, collect_actions_log};
use super::output::{Annotation, Check, CheckBucket};
use super::validation::validate_annotations;

const MAX_DIAGNOSTIC_WORKERS: usize = 4;

pub(super) fn collect_diagnostics(
    target: &Target,
    checks: &mut [Check],
    options: CheckDiagnosticsOptions,
    head_oid: &str,
    deadline: Instant,
    timeout_message: &str,
) -> Result<()> {
    let failed = checks
        .iter()
        .enumerate()
        .filter_map(|(index, check)| {
            matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel).then_some(index)
        })
        .collect::<Vec<_>>();
    if failed.is_empty() {
        return Ok(());
    }

    let progress = Progress::start(failed.len(), options.quiet);
    ensure_before_deadline(deadline, timeout_message)?;
    let context = DiagnosticContext {
        target,
        include_failed_logs: options.include_failed_logs,
        head_oid,
        deadline,
        timeout_message,
    };
    let diagnostic_jobs = failed
        .iter()
        .enumerate()
        .filter_map(|(position, &index)| {
            checks[index]
                .check_run_id
                .map(|check_run_id| DiagnosticJob {
                    index,
                    position,
                    check_run_id,
                    link: checks[index].link.clone(),
                })
        })
        .collect::<Vec<_>>();
    if diagnostic_worker_count(diagnostic_jobs.len()) == 0 {
        for index in failed {
            ensure_before_deadline(deadline, timeout_message)?;
            let check = &mut checks[index];
            let result =
                collect_one_diagnostic(&context, check.check_run_id, check.link.as_deref())?;
            apply_diagnostic_result(check, result, options.include_failed_logs);
            progress.complete_one();
        }
    } else {
        for &index in &failed {
            if checks[index].check_run_id.is_none() {
                let result = collect_one_diagnostic(&context, None, None)?;
                apply_diagnostic_result(&mut checks[index], result, options.include_failed_logs);
            }
        }
        let results =
            collect_diagnostics_parallel(&context, &diagnostic_jobs, failed.len(), &progress);
        for (index, result) in results {
            let result = result?;
            apply_diagnostic_result(&mut checks[index], result, options.include_failed_logs);
        }
    }
    ensure_before_deadline(deadline, timeout_message)?;
    Ok(())
}

struct DiagnosticJob {
    index: usize,
    position: usize,
    check_run_id: u64,
    link: Option<String>,
}

struct DiagnosticContext<'a> {
    target: &'a Target,
    include_failed_logs: bool,
    head_oid: &'a str,
    deadline: Instant,
    timeout_message: &'a str,
}

fn diagnostic_worker_count(job_count: usize) -> usize {
    if job_count < 2 {
        0
    } else {
        job_count.min(MAX_DIAGNOSTIC_WORKERS)
    }
}

struct DiagnosticResult {
    annotations: Vec<Annotation>,
    log: Option<Value>,
}

fn collect_one_diagnostic(
    context: &DiagnosticContext<'_>,
    check_run_id: Option<u64>,
    link: Option<&str>,
) -> Result<DiagnosticResult> {
    let annotations = match check_run_id {
        Some(id) => validate_annotations(&github::checks::annotations(
            context.target,
            id,
            context.deadline,
            context.timeout_message,
        )?)?,
        None => Vec::new(),
    };
    let log = if context.include_failed_logs {
        Some(if check_run_id.is_some() {
            collect_actions_log(
                context.target,
                link,
                check_run_id,
                context.head_oid,
                LOG_BYTE_LIMIT,
                LOG_LINE_LIMIT,
                context.deadline,
                context.timeout_message,
            )?
        } else {
            Value::Null
        })
    } else {
        None
    };
    Ok(DiagnosticResult { annotations, log })
}

fn apply_diagnostic_result(check: &mut Check, result: DiagnosticResult, include_failed_logs: bool) {
    check.annotations = Some(result.annotations);
    if include_failed_logs {
        check.log = result.log;
    }
}

fn collect_diagnostics_parallel(
    context: &DiagnosticContext<'_>,
    jobs: &[DiagnosticJob],
    total_progress: usize,
    progress: &Progress,
) -> Vec<(usize, Result<DiagnosticResult>)> {
    let next_job = Arc::new(Mutex::new(0usize));
    let stop = Arc::new(AtomicBool::new(false));
    let results = Arc::new(Mutex::new(Vec::with_capacity(jobs.len())));
    let mut is_job = vec![false; total_progress];
    for job in jobs {
        is_job[job.position] = true;
    }
    let completed = Arc::new(
        is_job
            .into_iter()
            .map(|is_job| AtomicBool::new(!is_job))
            .collect::<Vec<_>>(),
    );
    let next_completed = Arc::new(AtomicUsize::new(0));
    let worker_count = diagnostic_worker_count(jobs.len());
    mark_ordered_completion(&completed, &next_completed, total_progress, progress);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let next_job = Arc::clone(&next_job);
            let stop = Arc::clone(&stop);
            let results = Arc::clone(&results);
            let completed = Arc::clone(&completed);
            let next_completed = Arc::clone(&next_completed);
            scope.spawn(move || {
                while let Some(job_position) =
                    claim_next_diagnostic_job(&next_job, &stop, jobs.len())
                {
                    let job = &jobs[job_position];
                    let result = ensure_before_deadline(context.deadline, context.timeout_message)
                        .and_then(|()| {
                            collect_one_diagnostic(
                                context,
                                Some(job.check_run_id),
                                job.link.as_deref(),
                            )
                        });
                    let failed = result.is_err();
                    if failed {
                        stop.store(true, Ordering::Release);
                    }
                    results
                        .lock()
                        .expect("lock diagnostic results")
                        .push((job.index, result));
                    if !failed {
                        completed[job.position].store(true, Ordering::Release);
                        mark_ordered_completion(
                            &completed,
                            &next_completed,
                            total_progress,
                            progress,
                        );
                    }
                }
            });
        }
    });
    let mut results = {
        let mut shared_results = results.lock().expect("lock diagnostic results");
        std::mem::take(&mut *shared_results)
    };
    results.sort_unstable_by_key(|(index, _)| *index);
    results
}

fn claim_next_diagnostic_job(
    next_job: &Mutex<usize>,
    stop: &AtomicBool,
    job_count: usize,
) -> Option<usize> {
    let mut next_job = next_job.lock().expect("lock next diagnostic job");
    if stop.load(Ordering::Acquire) || *next_job >= job_count {
        return None;
    }
    let job_position = *next_job;
    *next_job += 1;
    Some(job_position)
}

fn mark_ordered_completion(
    completed: &[AtomicBool],
    next_completed: &AtomicUsize,
    total: usize,
    progress: &Progress,
) {
    loop {
        let next = next_completed.load(Ordering::Acquire);
        if next >= total || !completed[next].load(Ordering::Acquire) {
            break;
        }
        if next_completed
            .compare_exchange(next, next + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            progress.complete_one();
        }
    }
}

pub(super) fn diagnostic_deadline(timeout_seconds: u64) -> Option<Instant> {
    Instant::now().checked_add(Duration::from_secs(timeout_seconds))
}

fn ensure_before_deadline(deadline: Instant, timeout_message: &str) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    Err(Exit::runtime(&RuntimeError {
        kind: ErrorKind::Timeout,
        message: timeout_message.to_owned(),
        retryable: true,
        retry_after_seconds: None,
    }))
}

struct Progress {
    completed: Arc<AtomicUsize>,
    state: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Progress {
    fn start(total: usize, quiet: bool) -> Self {
        Self::start_with_interval(total, quiet, Duration::from_secs(15), None)
    }

    #[cfg(test)]
    fn start_for_test(
        total: usize,
        interval: Duration,
    ) -> (Self, std::sync::mpsc::Receiver<usize>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (
            Self::start_with_interval(total, false, interval, Some(sender)),
            receiver,
        )
    }

    fn start_with_interval(
        total: usize,
        quiet: bool,
        interval: Duration,
        report_sender: Option<std::sync::mpsc::Sender<usize>>,
    ) -> Self {
        let completed = Arc::new(AtomicUsize::new(0));
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        if quiet {
            return Self {
                completed,
                state,
                thread: None,
            };
        }

        writeln!(
            io::stderr(),
            "gh-loupe: collecting diagnostics for {total} failed checks"
        )
        .ok();
        let thread_completed = Arc::clone(&completed);
        let thread_state = Arc::clone(&state);
        let progress_thread = thread::spawn(move || {
            let started = Instant::now();
            let (lock, wake) = &*thread_state;
            let mut finished = lock.lock().expect("lock diagnostic progress");
            loop {
                let (next_finished, timeout) = wake
                    .wait_timeout(finished, interval)
                    .expect("wait for diagnostic progress");
                finished = next_finished;
                if *finished {
                    break;
                }
                if timeout.timed_out() {
                    let done = thread_completed.load(Ordering::Relaxed);
                    let elapsed = started.elapsed().as_secs();
                    writeln!(
                        io::stderr(),
                        "gh-loupe: diagnostics {done}/{total} complete; {elapsed}s elapsed"
                    )
                    .ok();
                    if let Some(sender) = &report_sender {
                        sender.send(done).ok();
                    }
                }
            }
        });
        Self {
            completed,
            state,
            thread: Some(progress_thread),
        }
    }

    fn complete_one(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        *lock.lock().expect("lock diagnostic progress") = true;
        wake.notify_one();
        if let Some(progress_thread) = self.thread.take() {
            progress_thread.join().expect("join diagnostic progress");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_with_zero_or_one_job_do_not_create_workers() {
        assert_eq!(diagnostic_worker_count(0), 0);
        assert_eq!(diagnostic_worker_count(1), 0);
        assert_eq!(diagnostic_worker_count(2), 2);
        assert_eq!(diagnostic_worker_count(10), MAX_DIAGNOSTIC_WORKERS);
    }

    #[test]
    fn diagnostic_job_claim_stops_atomically_after_failure() {
        let next_job = Mutex::new(0);
        let stop = AtomicBool::new(false);

        assert_eq!(claim_next_diagnostic_job(&next_job, &stop, 2), Some(0));
        stop.store(true, Ordering::Release);
        assert_eq!(claim_next_diagnostic_job(&next_job, &stop, 2), None);
    }

    #[test]
    fn progress_reports_ordered_completion_at_a_short_test_interval() {
        let (progress, reports) = Progress::start_for_test(3, Duration::from_millis(10));
        let completed = [
            AtomicBool::new(true),
            AtomicBool::new(false),
            AtomicBool::new(false),
        ];
        let next_completed = AtomicUsize::new(0);
        mark_ordered_completion(&completed, &next_completed, 3, &progress);

        assert_eq!(
            reports
                .recv_timeout(Duration::from_secs(1))
                .expect("progress report"),
            1
        );
    }
}
