//! Stream subprocess output to PostgreSQL without buffering a complete build in memory.
use anyhow::{Context, Result, bail};
use forge_deploy_db as db;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use sqlx::PgPool;
use std::process::Stdio;
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};
use uuid::Uuid;

struct ProcessGroup {
    child: Child,
    pid: Pid,
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // Also stop Git helpers and the Docker CLI if execution is cancelled or times out.
        let _ = killpg(self.pid, Signal::SIGKILL);
    }
}

pub async fn run(
    pool: &PgPool,
    id: Uuid,
    mut command: Command,
    failure: &'static str,
) -> Result<()> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = command
        .spawn()
        .context("Não foi possível iniciar Git ou Docker CLI")?;
    let pid = Pid::from_raw(child.id().context("Processo sem PID")? as i32);
    let mut stdout = child.stdout.take().context("stdout indisponível")?;
    let mut stderr = child.stderr.take().context("stderr indisponível")?;
    let mut process = ProcessGroup { child, pid };
    let mut out = [0_u8; 2048];
    let mut err = [0_u8; 2048];
    let mut out_open = true;
    let mut err_open = true;
    while out_open || err_open {
        tokio::select! {
            count = stdout.read(&mut out), if out_open => {
                let count = count?;
                out_open = count != 0;
                if count != 0 { db::append_log(pool, id, &String::from_utf8_lossy(&out[..count])).await?; }
            },
            count = stderr.read(&mut err), if err_open => {
                let count = count?;
                err_open = count != 0;
                if count != 0 { db::append_log(pool, id, &String::from_utf8_lossy(&err[..count])).await?; }
            },
        }
    }
    let status = process.child.wait().await?;
    if !status.success() {
        bail!(failure);
    }
    Ok(())
}
