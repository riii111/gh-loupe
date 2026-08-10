pub struct Target {
    pub repository: String,
    pub number: String,
}

#[derive(Clone, Copy)]
pub struct CheckDiagnosticsOptions {
    pub failed_diagnostics: bool,
    pub include_failed_logs: bool,
    pub timeout_seconds: u64,
    pub quiet: bool,
}
