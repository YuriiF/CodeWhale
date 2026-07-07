//! Configured provider/model lake facade (#3830, Wave 5b).
//!
//! Single seam over the bundled Models.dev catalog and the configured-provider
//! predicate shared with `/provider`. Pickers, hotbar route slots,
//! [`crate::model_inventory::ModelInventory`], slash completions, and subagent
//! validation should read model lists from here instead of the legacy hardcoded
//! table in [`crate::config::model_completion_names_for_provider`].

use std::sync::OnceLock;

use codewhale_config::catalog::{CatalogSnapshot, bundled_catalog_offerings};

use crate::config::{
    ApiProvider, Config, model_completion_names_for_provider, provider_is_configured_for_active,
};

static BUNDLED_SNAPSHOT: OnceLock<CatalogSnapshot> = OnceLock::new();

fn bundled_snapshot() -> &'static CatalogSnapshot {
    BUNDLED_SNAPSHOT.get_or_init(|| CatalogSnapshot {
        offerings: bundled_catalog_offerings(),
    })
}

/// Maps an [`ApiProvider`] to its bundled-catalog provider id.
fn catalog_provider_id(provider: ApiProvider) -> &'static str {
    match provider {
        ApiProvider::DeepseekCN | ApiProvider::DeepseekAnthropic => "deepseek",
        ApiProvider::SiliconflowCn => "siliconflow",
        _ => provider.as_str(),
    }
}

fn push_unique_model(models: &mut Vec<String>, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    if !models
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(model))
    {
        models.push(model.to_string());
    }
}

fn catalog_models_from_offerings<'a>(
    offerings: impl IntoIterator<Item = &'a codewhale_config::catalog::CatalogOffering>,
) -> Vec<String> {
    let mut rows: Vec<_> = offerings.into_iter().collect();
    rows.sort_by(|left, right| {
        right
            .default_for_provider
            .cmp(&left.default_for_provider)
            .then_with(|| left.wire_model_id.cmp(&right.wire_model_id))
    });
    let mut models = Vec::new();
    for row in rows {
        push_unique_model(&mut models, &row.wire_model_id);
    }
    models
}

/// Bundled-catalog model ids for one provider.
///
/// Returns provider wire ids from [`bundled_catalog_offerings`]. Providers not
/// yet represented in the bundled asset fall back to the legacy hardcoded table
/// so routing surfaces stay usable until the asset catches up.
#[must_use]
pub fn all_catalog_models_for_provider(provider: ApiProvider) -> Vec<String> {
    let catalog_id = catalog_provider_id(provider);
    let mut models =
        catalog_models_from_offerings(bundled_snapshot().offerings_for_provider(catalog_id));
    if models.is_empty() {
        for model in model_completion_names_for_provider(provider) {
            push_unique_model(&mut models, model);
        }
    }
    models
}

/// Count of bundled-catalog models for one provider (catalog view / dashboard).
#[must_use]
pub fn catalog_model_count_for_provider(provider: ApiProvider) -> usize {
    all_catalog_models_for_provider(provider).len()
}

/// Providers the user has set up — active provider, working credentials/OAuth,
/// or an explicit `[providers.<name>]` entry (#3830).
#[must_use]
pub fn configured_providers(config: &Config, active: ApiProvider) -> Vec<ApiProvider> {
    ApiProvider::sorted_for_display()
        .into_iter()
        .filter(|provider| provider_is_configured_for_active(config, *provider, active))
        .collect()
}

/// Catalog models for providers that qualify as configured for `active`.
#[must_use]
pub fn models_for_provider(
    config: &Config,
    active: ApiProvider,
    provider: ApiProvider,
) -> Vec<String> {
    if provider_is_configured_for_active(config, provider, active) {
        all_catalog_models_for_provider(provider)
    } else {
        Vec::new()
    }
}

/// Every built-in provider that carries at least one bundled-catalog row.
#[must_use]
#[allow(dead_code)]
pub fn all_catalog_providers() -> Vec<ApiProvider> {
    let mut seen = Vec::new();
    for offering in &bundled_snapshot().offerings {
        if let Some(provider) = ApiProvider::parse(&offering.provider)
            && !seen.contains(&provider)
        {
            seen.push(provider);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_TOGETHER_FLASH_MODEL, DEFAULT_TOGETHER_MODEL};

    #[test]
    fn together_catalog_includes_flash_from_bundled_asset() {
        let models = all_catalog_models_for_provider(ApiProvider::Together);
        assert!(
            models.contains(&DEFAULT_TOGETHER_MODEL.to_string()),
            "missing Together pro: {models:?}"
        );
        assert!(
            models.contains(&DEFAULT_TOGETHER_FLASH_MODEL.to_string()),
            "missing Together flash: {models:?}"
        );
    }

    #[test]
    fn configured_providers_matches_provider_predicate() {
        let config = Config::default();
        let active = ApiProvider::Deepseek;
        let expected: Vec<_> = ApiProvider::sorted_for_display()
            .into_iter()
            .filter(|provider| {
                crate::config::provider_is_configured_for_active(&config, *provider, active)
            })
            .collect();
        assert_eq!(configured_providers(&config, active), expected);
    }

    #[test]
    fn models_for_provider_filters_unconfigured_gateways() {
        let _env_lock = crate::test_support::lock_test_env();
        let _together = crate::test_support::EnvVarGuard::remove("TOGETHER_API_KEY");
        let config = Config::default();
        assert!(
            models_for_provider(&config, ApiProvider::Deepseek, ApiProvider::Together).is_empty()
        );
        assert!(
            !models_for_provider(&config, ApiProvider::Deepseek, ApiProvider::Deepseek).is_empty()
        );
    }
}
