pub mod common;
pub mod manager;
pub mod matching;

pub use manager::*;

use crate::error::AppError;
use crate::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult};
use crate::web_session::{SessionUse, SiteSpec};
use async_trait::async_trait;

#[async_trait]
pub trait AnimeProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether a dub request can ever be answered with dubbed audio.
    fn supports_dub(&self) -> bool {
        false
    }

    /// Providers served from the same site share this key, so one unreachable site is cooled
    /// down once rather than once per provider.
    fn site(&self) -> &'static str {
        self.name()
    }

    /// Providers that are addressed by AniList id and episode number have no episode list of
    /// their own; they return the episode to ask for here and `get_episodes` stays empty.
    fn episode_by_number(&self, _query: &AnimeQuery, _number: f64) -> Option<EpisodeItem> {
        None
    }

    /// The site this provider can also ask through the in-app browser when the plain client
    /// is refused. Such providers implement the `_with` methods, which say how far they may go.
    fn session_site(&self) -> Option<&'static SiteSpec> {
        None
    }

    /// Whether a refusal by the site's anti-bot page may be answered through the in-app
    /// browser, so the manager retries this provider with permission to start a session.
    fn may_use_session(&self) -> bool {
        self.session_site().is_some()
    }

    /// True when `get_sources` only returns streams whose first entry it has already checked
    /// for playability, so the manager does not check them a second time.
    fn verifies_streams(&self) -> bool {
        false
    }

    /// Identifies the provider's cached episode lists. A provider whose code can change while
    /// the app runs returns a different key for each version.
    fn cache_key(&self) -> String {
        self.name().to_string()
    }

    async fn get_episodes(&self, query: &AnimeQuery) -> Result<ProviderData, AppError>;

    async fn get_sources(
        &self,
        episode: &EpisodeItem,
        category: Category,
    ) -> Result<SourcesResult, AppError>;

    /// `get_episodes`, told whether an in-app browser session may be created for it.
    async fn get_episodes_with(
        &self,
        query: &AnimeQuery,
        _access: SessionUse,
    ) -> Result<ProviderData, AppError> {
        self.get_episodes(query).await
    }

    /// `get_sources`, told whether an in-app browser session may be created for it.
    async fn get_sources_with(
        &self,
        episode: &EpisodeItem,
        category: Category,
        _access: SessionUse,
    ) -> Result<SourcesResult, AppError> {
        self.get_sources(episode, category).await
    }
}
