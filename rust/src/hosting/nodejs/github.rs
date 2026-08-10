//! `gddy hosting nodejs github` — manage GitHub connections.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec,
    Tier,
};

use crate::hosting::nodejs::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_GITHUB_EXECUTE as GITHUB_EXECUTE;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("github", "Manage GitHub connections"))
        .with_command(status_command())
        .with_command(repos_command())
        .with_command(branches_command())
}

fn status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("status", "Show GitHub connection status")
            .with_long("Show whether GitHub is connected to a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .get_github_status(&app_id)
                .await
                .map_err(client_err)?;
            let connected = data
                .get("connected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut actions = Vec::new();
            if connected {
                actions.push(
                    next_action(
                        "hosting nodejs github repos --app-id <app-id>",
                        "List accessible GitHub repositories",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct GithubReposArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Repositories per page (1-100).
    #[arg(long = "per-page", value_name = "N", value_parser = clap::value_parser!(u32).range(1..=100))]
    per_page: Option<u32>,

    /// Sort order (created, updated, pushed, full_name).
    #[arg(long, value_name = "SORT", value_parser = ["created", "updated", "pushed", "full_name"])]
    sort: Option<String>,
}

fn repos_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GithubReposArgs, _, _, _>(
        CommandSpec::from_args::<GithubReposArgs>("repos", "List accessible GitHub repositories")
            .with_long("List GitHub repositories accessible to the connected account.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: GithubReposArgs| async move {
            let app_id = args.app_id;
            let per_page = args.per_page;
            let sort = args.sort;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .list_github_repos(&app_id, per_page, sort.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs github branches --app-id <app-id> --owner <owner> --repo <repo>",
                    "List branches for a repository",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("owner", NextActionParam::required())
                .with_param("repo", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct GithubBranchesArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Repository owner (user or org).
    #[arg(long, value_name = "OWNER")]
    owner: String,

    /// Repository name.
    #[arg(long, value_name = "REPO")]
    repo: String,

    /// Branches per page (1-100).
    #[arg(long = "per-page", value_name = "N", value_parser = clap::value_parser!(u32).range(1..=100))]
    per_page: Option<u32>,
}

fn branches_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GithubBranchesArgs, _, _, _>(
        CommandSpec::from_args::<GithubBranchesArgs>(
            "branches",
            "List branches for a GitHub repository",
        )
        .with_long("List branches for a GitHub repository accessible to the connected account.")
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: GithubBranchesArgs| async move {
            let app_id = args.app_id;
            let owner = args.owner;
            let repo = args.repo;
            let per_page = args.per_page;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .list_github_branches(&app_id, &owner, &repo, per_page)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs source git --app-id <app-id> --repo <owner/repo> --branch <branch>",
                    "Import source from this repository",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("repo", NextActionParam::value(format!("{owner}/{repo}")))
                .with_param("branch", NextActionParam::required()),
            ]))
        },
    )
}
