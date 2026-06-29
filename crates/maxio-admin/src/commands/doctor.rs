use crate::cli::CommandContext;
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;
use crate::output::emit;
use maxio::storage::filesystem::FilesystemStorage;
use maxio::storage::keys::Keyring;
use maxio::storage::quota::QuotaLimits;
use serde_json::json;
use std::sync::Arc;

pub async fn run(data_dir: Option<String>, ctx: CommandContext) -> Result<()> {
    if let Some(data_dir) = data_dir {
        return run_local(&data_dir, ctx.json).await;
    }

    let session = AdminSession::connect(ctx.profile)?;
    run_remote(
        ctx.json,
        &ctx.profile_name,
        &session,
        "doctor",
        session.doctor(),
    )
    .await
}

async fn run_local(data_dir: &str, json: bool) -> Result<()> {
    let keyring = Arc::new(Keyring::load(data_dir, None).await?);
    let storage = FilesystemStorage::new(
        data_dir,
        false,
        10 * 1024 * 1024,
        0,
        keyring.clone(),
        QuotaLimits::from_config(0, 0),
        false,
    )
    .await?;

    let readiness = storage.check_readiness().await;
    let disk_result = storage.check_upload_start(None);
    let keyring_ok = keyring.is_usable();

    let readiness_detail = match &readiness {
        Ok(()) => "data directory writable and keyring usable".to_string(),
        Err(msg) => msg.clone(),
    };
    let disk_ok = disk_result.is_ok();
    let disk_detail = if disk_ok {
        "disk reserve satisfied".to_string()
    } else {
        disk_result.unwrap_err().to_string()
    };

    let checks = vec![
        json!({
            "name": "readiness",
            "ok": readiness.is_ok(),
            "detail": readiness_detail,
        }),
        json!({
            "name": "disk_reserve",
            "ok": disk_ok,
            "detail": disk_detail,
        }),
        json!({
            "name": "keyring",
            "ok": keyring_ok,
            "detail": if keyring_ok {
                format!("SSE-S3 keyring usable (active key id {})", keyring.active_id())
            } else {
                "SSE-S3 keyring has no keys".into()
            },
        }),
    ];
    let ok = checks.iter().all(|c| c["ok"].as_bool().unwrap_or(false));
    emit(
        json,
        &json!({ "ok": ok, "checks": checks, "data_dir": data_dir }),
    )?;
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}
