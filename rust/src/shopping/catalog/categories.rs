use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde::{Deserialize, Serialize};
use serde_json::json;
use shopping_client::types::{
    CatalogSearchSearchRequest, CatalogSearchSearchResponse, PaginationRequest,
    SearchCatalogResponse, SearchRequest,
};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::{ClientError, decode};
use crate::shopping::common::{client_err, make_client, reject_response_errors};
use crate::shopping::human::CATALOG_CATEGORIES_VIEW_ID;

const CATALOG_CATEGORY_LIMIT: std::num::NonZeroU64 =
    std::num::NonZeroU64::new(50).expect("CATALOG_CATEGORY_LIMIT is nonzero");
const CACHE_FILE_NAME: &str = "shopping-catalog-categories.json";
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cache {
    cached_at: DateTime<Utc>,
    categories: Vec<String>,
}

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Fetch current categories instead of using a cached result.
    #[arg(long)]
    refresh: bool,
}

output_schema!(CatalogCategoriesOutput {
    "categories": "[]string";
});

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("categories", "List supported product categories")
            .with_long(
                "List the supported product categories currently available in the Shopping catalog. \
                 The list is derived from up to 50 catalog products and cached for six hours. Use \
                 --refresh to fetch current categories.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogCategoriesOutput>()
            .with_view_id(CATALOG_CATEGORIES_VIEW_ID),
        |ctx, args: Args| async move {
            let cache_path = cache_path(&ctx.middleware.env);
            let cached_categories = (!args.refresh)
                .then(|| cache_path.as_deref().and_then(|path| load_fresh_cache(path, Utc::now())))
                .flatten();
            let categories = match cached_categories {
                Some(categories) => categories,
                None => refresh_categories(&ctx, cache_path.as_deref()).await?,
            };
            Ok(CommandResult::new(json!({"categories": categories})))
        },
    )
}

async fn refresh_categories(
    ctx: &cli_engine::CommandContext,
    cache_path: Option<&Path>,
) -> cli_engine::Result<Vec<String>> {
    let client = make_client(ctx).await?;
    let request = CatalogSearchSearchRequest {
        pagination: Some(PaginationRequest {
            limit: CATALOG_CATEGORY_LIMIT,
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = decode::<SearchCatalogResponse>(
        client
            .search_catalog()
            .body(SearchRequest(request))
            .send()
            .await,
    )
    .await
    .map_err(client_err)?
    .ok_or_else(|| client_err(ClientError::EmptyResponse))?;
    let response = match response {
        SearchCatalogResponse::SearchResponse(response) => response.0,
        SearchCatalogResponse::ErrorResponse(payload) => {
            return Err(client_err(ClientError::UnexpectedErrorPayload(
                payload.into(),
            )));
        }
    };
    reject_response_errors(&response.messages)?;
    let categories = categories(&response);
    if let Some(path) = cache_path
        && let Err(error) = save_cache(path, &categories, Utc::now())
    {
        tracing::warn!(%error, "could not cache Shopping catalog categories");
    }
    Ok(categories)
}

fn cache_path(env: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|dir| {
        let filename = if env == "prod" {
            CACHE_FILE_NAME.to_owned()
        } else {
            format!("shopping-catalog-categories-{}.json", cache_env_name(env))
        };
        dir.join("gddy").join(filename)
    })
}

fn cache_env_name(env: &str) -> String {
    env.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn load_fresh_cache(path: &Path, now: DateTime<Utc>) -> Option<Vec<String>> {
    let cache = serde_json::from_str::<Cache>(&std::fs::read_to_string(path).ok()?).ok()?;
    (now.signed_duration_since(cache.cached_at).to_std().ok()? < CACHE_TTL)
        .then_some(cache.categories)
}

fn save_cache(
    path: &Path,
    categories: &[String],
    cached_at: DateTime<Utc>,
) -> cli_engine::Result<()> {
    let cache = Cache {
        cached_at,
        categories: categories.to_vec(),
    };
    let contents = serde_json::to_string_pretty(&cache).map_err(|error| {
        cli_engine::CliCoreError::message(format!(
            "failed to serialize Shopping category cache: {error}"
        ))
    })?;
    cli_engine::fs::write_string_atomic(path, &contents)
}

fn categories(response: &CatalogSearchSearchResponse) -> Vec<String> {
    response
        .products
        .iter()
        .flat_map(|product| product.categories.iter())
        .filter_map(|category| category.value.as_deref())
        .filter(|category| !category.eq_ignore_ascii_case("domain"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use shopping_client::types::{Category, Product};

    use super::{
        CACHE_TTL, Cache, CatalogSearchSearchResponse, cache_env_name, categories,
        load_fresh_cache, save_cache,
    };

    fn product_with_categories(categories: &[&str]) -> Product {
        Product {
            categories: categories
                .iter()
                .map(|value| Category {
                    value: Some((*value).to_owned()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn derives_sorted_unique_non_domain_categories_from_products() {
        let response = CatalogSearchSearchResponse {
            products: vec![
                product_with_categories(&["webHosting", "email"]),
                product_with_categories(&["email", "domain"]),
                product_with_categories(&["DOMAIN", "sslCertificate"]),
                Product::default(),
            ],
            ..Default::default()
        };
        assert_eq!(
            categories(&response),
            vec!["email", "sslCertificate", "webHosting"]
        );
    }

    #[test]
    fn cache_filename_sanitizes_the_environment_name() {
        assert_eq!(cache_env_name("test-local/1"), "test_local_1");
    }

    #[test]
    fn returns_categories_from_a_fresh_cache() {
        let directory = tempfile::tempdir().expect("temporary cache directory");
        let path = directory.path().join("categories.json");
        let now = chrono::Utc::now();
        let categories = vec!["email".to_owned()];
        save_cache(&path, &categories, now).expect("save category cache");

        assert_eq!(load_fresh_cache(&path, now), Some(categories));
    }

    #[test]
    fn ignores_expired_or_malformed_caches() {
        let directory = tempfile::tempdir().expect("temporary cache directory");
        let path = directory.path().join("categories.json");
        let now = chrono::Utc::now();
        let expired = Cache {
            cached_at: now
                - TimeDelta::from_std(CACHE_TTL).expect("TTL is valid")
                - TimeDelta::seconds(1),
            categories: vec!["email".to_owned()],
        };
        std::fs::write(
            &path,
            serde_json::to_string(&expired).expect("serialize cache"),
        )
        .expect("write expired cache");
        assert_eq!(load_fresh_cache(&path, now), None);

        std::fs::write(&path, "not JSON").expect("write malformed cache");
        assert_eq!(load_fresh_cache(&path, now), None);
    }
}
