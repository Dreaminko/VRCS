mod download;
mod manager;
mod manifest;
mod migration;
mod package;

pub(super) use download::download_file;

pub(super) use manifest::{package_spec, PACKAGES};
pub(super) use manifest::{AssetSpec, PackageSpec};
pub(super) use migration::prepare_migration;
pub(super) use package::{activate_staging, is_installed, package_dir, staging_dir};

#[cfg(test)]
pub(super) use download::download_into_staging;
pub(crate) fn is_supported(id: &str) -> bool {
    manifest::PACKAGES.iter().any(|spec| spec.id == id)
}
