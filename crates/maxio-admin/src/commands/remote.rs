use crate::client::AdminSession;
use crate::error::{AdminError, Result};
use crate::output::{emit, emit_stub};
use serde_json::Value;

pub async fn run_remote(
    json: bool,
    profile_name: &str,
    session: &AdminSession,
    command: &str,
    fetch: impl std::future::Future<Output = Result<Value>>,
) -> Result<()> {
    match fetch.await {
        Ok(value) => emit(json, &value),
        Err(AdminError::ApiNotAvailable { .. }) => {
            emit_stub(json, command, session.endpoint(), profile_name)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}
