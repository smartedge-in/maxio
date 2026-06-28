use crate::cli::ConfigAction;
use crate::config::default_config_path;
use crate::error::Result;
use crate::output::emit_message;

pub fn run(action: ConfigAction, json: bool) -> Result<()> {
    match action {
        ConfigAction::Path => {
            let path = default_config_path();
            emit_message(
                json,
                &format!(
                    "{}\n\nExample:\n  default_profile = \"local\"\n\n  [profiles.local]\n  endpoint = \"http://127.0.0.1:9000\"\n  admin_token = \"change-me\"",
                    path.display()
                ),
            );
        }
    }
    Ok(())
}
