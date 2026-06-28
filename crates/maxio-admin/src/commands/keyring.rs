use crate::cli::KeyringCommand;
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;
use crate::output::emit;
use maxio::storage::keys;
use std::path::PathBuf;

pub async fn run(
    cmd: KeyringCommand,
    profile: Option<String>,
    endpoint: Option<String>,
    json: bool,
    config: Option<PathBuf>,
) -> Result<()> {
    match cmd {
        KeyringCommand::List => {
            let ctx = crate::cli::build_context(profile, endpoint, json, config).await?;
            let session = AdminSession::connect(ctx.profile)?;
            run_remote(
                ctx.json,
                &ctx.profile_name,
                &session,
                "keyring list",
                session.keyring_list(),
            )
            .await
        }
        KeyringCommand::Rotate { data_dir } => {
            let result = keys::rotate(&data_dir).await?;
            let value = serde_json::json!({
                "status": "rotated",
                "data_dir": data_dir,
                "new_active_id": result.new_active_id,
                "previous_active_id": result.previous_active_id,
                "total_keys": result.total_keys,
                "message": "Restart the server to encrypt new objects with the new active key."
            });
            emit(json, &value)?;
            Ok(())
        }
    }
}
