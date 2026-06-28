use crate::cli::KeyringCommand;
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;
use crate::output::emit_message;
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
            emit_message(
                json,
                &format!(
                    "keyring rotate is local-only (filesystem access required).\n\
                     Stub: run `maxio keyring rotate --data-dir {}` until this command \
                     is wired to offline keyring helpers (P2-12).",
                    data_dir
                ),
            );
            Ok(())
        }
    }
}

