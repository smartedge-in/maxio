use crate::cli::{BucketsCommand, CommandContext};
use crate::client::AdminSession;
use crate::commands::remote::run_remote;
use crate::error::Result;

pub async fn run(cmd: BucketsCommand, ctx: CommandContext) -> Result<()> {
    let session = AdminSession::connect(ctx.profile)?;
    match cmd {
        BucketsCommand::List => {
            run_remote(
                ctx.json,
                &ctx.profile_name,
                &session,
                "buckets list",
                session.list_buckets(),
            )
            .await
        }
        BucketsCommand::Head { name } => {
            run_remote(
                ctx.json,
                &ctx.profile_name,
                &session,
                &format!("buckets head {name}"),
                session.head_bucket(&name),
            )
            .await
        }
    }
}