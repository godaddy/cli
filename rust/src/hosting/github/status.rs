use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingGitHubProfile, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_GITHUB_READ as GH_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<(), _, _, _>(
        CommandSpec::new("status", "Show the GitHub connection status")
            .with_long(
                "Show whether a GitHub account is connected to your GoDaddy hosting profile. \
                 Connection is account-level, not per-application.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GH_READ])
            .with_output_schema::<HostingGitHubProfile>(),
        |ctx, _args: ()| async move {
            let client = make_client(&ctx, &[GH_READ]).await?;
            let data = client.get_github_connection().await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![next_action(
                "hosting github repos",
                "Browse connected repositories",
            )]))
        },
    )
}
