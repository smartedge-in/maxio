use crate::cli::CommandContext;
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;

pub async fn run(ctx: CommandContext) -> Result<()> {
    let session = AdminSession::connect(ctx.profile)?;
    run_remote(
        ctx.json,
        &ctx.profile_name,
        &session,
        "status",
        session.status(),
    )
    .await
}
