use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::email::{client_err, make_client};
use crate::scopes::EMAIL_READ;

#[derive(Debug, Clone, clap::Args)]
struct GetArgs {
    /// The mailbox ID to fetch.
    mailbox_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GetArgs, _, _, _>(
        CommandSpec::from_args::<GetArgs>("get", "Get a mailbox by ID")
            .with_system("email")
            .with_tier(Tier::Read)
            .with_scopes(&[EMAIL_READ]),
        |ctx, args: GetArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let data = client
                .get_mailbox(&args.mailbox_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }
}
