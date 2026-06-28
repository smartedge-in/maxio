use crate::cli::{CommandContext, HousekeepingAction};
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;

pub async fn run(action: HousekeepingAction, ctx: CommandContext) -> Result<()> {
    let session = AdminSession::connect(ctx.profile)?;
    match action {
        HousekeepingAction::Run => {
            run_remote(
                ctx.json,
                &ctx.profile_name,
                &session,
                "housekeeping run",
                session.housekeeping_run(),
            )
            .await
        }
    }
}