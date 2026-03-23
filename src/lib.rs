use zed_extension_api::{self as zed, serde_json, settings::LspSettings, LanguageServerId, Result};

struct FortyTwoHeaderExtension;

impl zed::Extension for FortyTwoHeaderExtension {
    fn new() -> Self {
        FortyTwoHeaderExtension
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.path);

        Ok(zed::Command {
            command: binary.unwrap_or_else(|| "header-42-lsp".to_string()),
            args: vec![],
            env: Default::default(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|s| s.initialization_options))
    }
}

zed::register_extension!(FortyTwoHeaderExtension);
