use tracing::debug;
use tracing::error;
use tracing::info;

use crate::config::VpnConfig;
use crate::transport::Transport;

pub async fn run_post_up(transport: &Transport, config: &VpnConfig) -> anyhow::Result<()> {
    if config.post_up.is_empty() {
        debug!("No PostUp commands to execute");
        return Ok(());
    }

    info!("Running {} PostUp command(s)", config.post_up.len());

    for (i, cmd) in config.post_up.iter().enumerate() {
        info!("PostUp [{}/{}]: {}", i + 1, config.post_up.len(), cmd);

        if let Err(e) = transport.exec_success(cmd).await {
            error!("PostUp command failed: {}", cmd);
            return Err(e);
        }
    }

    info!("All PostUp commands completed successfully");
    Ok(())
}

pub async fn run_pre_down(transport: &Transport, config: &VpnConfig) {
    if config.pre_down.is_empty() {
        debug!("No PreDown commands to execute");
        return;
    }

    info!("Running {} PreDown command(s)", config.pre_down.len());

    for (i, cmd) in config.pre_down.iter().enumerate() {
        info!("PreDown [{}/{}]: {}", i + 1, config.pre_down.len(), cmd);

        match transport.exec(cmd).await {
            Ok(result) if result.exit_code == 0 => {
                debug!("PreDown command succeeded: {}", cmd);
            }
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);
                error!(
                    "PreDown command failed (exit {}): {} - stdout={}, stderr={}",
                    result.exit_code,
                    cmd,
                    stdout.trim(),
                    stderr.trim()
                );
            }
            Err(e) => {
                error!("PreDown command error: {} - {}", cmd, e);
            }
        }
    }

    info!("PreDown commands completed");
}
