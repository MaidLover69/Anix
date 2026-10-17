use crate::clients::anilist::AniListClient;
use crate::clients::http::build_http_client;
use crate::clients::imdb::ImdbClient;
use crate::clients::mal::MalClient;
use crate::plugins::registry::PluginRegistry;
use crate::providers::manager::ProviderManager;
use sqlx::SqlitePool;

pub struct AppState {
    pub pool: SqlitePool,
    pub client: reqwest::Client,
    pub anilist_client: AniListClient,
    pub mal_client: MalClient,
    pub imdb_client: ImdbClient,
    /// Installed streaming plugins; the provider manager asks it for its providers.
    pub plugins: PluginRegistry,
    pub provider_manager: ProviderManager,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        let client = build_http_client();
        let anilist_client = AniListClient::new(client.clone());
        let mal_client = MalClient::new(client.clone());
        let imdb_client = ImdbClient::new(client.clone());
        let plugins = PluginRegistry::open_default();
        let provider_manager = {
            let plugins = plugins.clone();
            ProviderManager::with_source_and_languages(
                std::sync::Arc::new(move || plugins.providers()),
                std::sync::Arc::new(crate::commands::settings::language_prefs),
            )
        };

        Self {
            pool,
            client,
            anilist_client,
            mal_client,
            imdb_client,
            plugins,
            provider_manager,
        }
    }
}
