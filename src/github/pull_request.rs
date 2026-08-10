use serde_json::Value;

use crate::error::Result;
use crate::model::Target;

use super::cli;

pub fn checks(target: &Target, required: bool) -> Result<Value> {
    let mut args = vec![
        "pr",
        "checks",
        &target.number,
        "--repo",
        &target.repository,
        "--json",
        "name,state,bucket,link,workflow,startedAt,completedAt",
    ];
    if required {
        args.push("--required");
        return cli::json_runtime_or_empty(args, None, true, "no required checks reported on ");
    }
    cli::json_runtime(args, None, true)
}
