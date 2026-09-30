mod github;
mod status;
mod upload;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("source", "Manage application source code").with_long(
            "Deploy code to your app's PREVIEW environment. Upload a local zip \
             (`hosting source upload`), then check import status. `hosting source github` \
             re-imports from a repo already linked in the Node.js Hosting UI \
             (`source` is GitHub on `app get`). After a successful import, use \
             `hosting deployment publish` to deploy to PUBLISH.",
        ),
    )
    .with_command(upload::command())
    .with_command(status::command())
    .with_command(github::command())
}
