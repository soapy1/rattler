//! Source enum and `RepoDataSource` trait for custom repodata providers.

use std::sync::Arc;

use rattler_conda_types::{Channel, PackageName, Platform, RepoDataRecord};

use super::{
    GatewayError,
    subdir::{PackageRecords, SubdirClient, extract_unique_deps_split},
};
use crate::{
    Reporter,
    sparse::{PackageFormatSelection, SparseRepoData},
};

/// A source of repodata records for a specific subdirectory.
///
/// Implement this trait to provide custom repodata records without
/// going through traditional channel URLs. The gateway will call
/// these methods for each platform in the query.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait RepoDataSource: Send + Sync {
    /// Fetch records for a specific package name and platform.
    ///
    /// This method is called by the gateway when it needs repodata records
    /// for a particular package. The platform parameter indicates which
    /// subdirectory the gateway is querying for.
    ///
    /// The gateway caches what it gets back once per package name and then
    /// narrows it to whatever selection each query asked for, so it always
    /// requests [`PackageFormatSelection::Both`] here rather than the
    /// caller's selection. A source is free to return every format it has;
    /// anything it drops is dropped for every query. One consequence today:
    /// `.whl` records are not part of `Both`, so a source that returns them
    /// will not see them surface under
    /// [`PackageFormatSelection::PreferCondaWithWhl`].
    async fn fetch_package_records(
        &self,
        platform: Platform,
        name: &PackageName,
        package_format_selection: PackageFormatSelection,
    ) -> Result<Vec<Arc<RepoDataRecord>>, GatewayError>;

    /// Return all available package names for the given platform.
    ///
    /// This is used by the gateway to know which packages are available
    /// in this source for a given platform/subdirectory.
    fn package_names(&self, platform: Platform) -> Vec<String>;
}

/// A source of repodata, either a channel or a custom source.
///
/// This enum allows the [`Gateway::query()`](super::Gateway::query) method
/// to accept both traditional channels custom repodata sources and sparse repodata.
#[derive(Clone)]
pub enum Source {
    /// A traditional conda channel (expanded to all requested platforms).
    Channel(Channel),

    /// A custom repodata source (provides records for requested platforms).
    Custom(Arc<dyn RepoDataSource>),

    /// A sparse repodata source (provides records for requested platforms from sparse
    /// repodata). Each entry represents a different subdir.
    SparseRepoData(Vec<Arc<SparseRepoData>>),
}

impl From<Channel> for Source {
    fn from(channel: Channel) -> Self {
        Source::Channel(channel)
    }
}

impl From<Arc<dyn RepoDataSource>> for Source {
    fn from(source: Arc<dyn RepoDataSource>) -> Self {
        Source::Custom(source)
    }
}

impl From<Arc<SparseRepoData>> for Source {
    fn from(source: Arc<SparseRepoData>) -> Self {
        Source::SparseRepoData(vec![source])
    }
}

impl From<Vec<Arc<SparseRepoData>>> for Source {
    fn from(sources: Vec<Arc<SparseRepoData>>) -> Self {
        Source::SparseRepoData(sources)
    }
}

/// Adapts a [`RepoDataSource`] to the internal [`SubdirClient`] trait
/// for a specific platform.
///
/// This adapter is used internally by the gateway to treat custom sources
/// the same way as channel subdirectories.
pub(super) struct CustomSourceClient {
    source: Arc<dyn RepoDataSource>,
    platform: Platform,
}

impl CustomSourceClient {
    /// Create a new adapter for the given source and platform.
    pub fn new(source: Arc<dyn RepoDataSource>, platform: Platform) -> Self {
        Self { source, platform }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl SubdirClient for CustomSourceClient {
    async fn fetch_package_records(
        &self,
        name: &PackageName,
        _reporter: Option<&dyn Reporter>,
    ) -> Result<PackageRecords, GatewayError> {
        // The gateway narrows the cached set per query, so ask the source for
        // as much as it can give. `Both` is the widest selection that keeps
        // every conda variant; see the note on `RepoDataSource` about `.whl`.
        let records = self
            .source
            .fetch_package_records(self.platform, name, PackageFormatSelection::Both)
            .await?;
        let (unique_base_deps, unique_extra_deps) =
            extract_unique_deps_split(records.iter().map(|r| &**r));
        Ok(PackageRecords {
            records,
            unique_base_deps,
            unique_extra_deps,
        })
    }

    fn package_names(&self) -> Vec<String> {
        self.source.package_names(self.platform)
    }
}
