//! Running the selected Bun. On Unix the shim replaces itself with Bun
//! (exec), so signals, exit codes and the process tree are Bun's own. Windows
//! has no exec: the shim runs Bun as a child, ignores Ctrl-C itself (the child
//! receives it from the console), and exits with the child's code.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub fn exec(binary: &Path, args: Vec<OsString>) -> Result<i32> {
    let mut command = Command::new(binary);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(error).with_context(|| format!("running {}", binary.display()))
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .with_context(|| format!("running {}", binary.display()))?;
        Ok(status.code().unwrap_or(1))
    }
}
