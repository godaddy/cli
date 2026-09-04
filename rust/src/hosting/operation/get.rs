use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingAppOperation, client_err, make_client};
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct OperationGetArgs {
    /// Operation ID.
    #[arg(long = "operation-id", value_name = "OPERATION_ID")]
    operation_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OperationGetArgs, _, _, _>(
        CommandSpec::from_args::<OperationGetArgs>("get", "Get an operation")
            .with_long(
                "Poll an async operation by ID. Operations are returned by `hosting app create` \
                 and `hosting deployment publish`. Keep polling until status is COMPLETED or FAILED.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: OperationGetArgs| async move {
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client
                .get_operation(&args.operation_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}
