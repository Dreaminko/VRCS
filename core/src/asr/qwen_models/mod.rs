mod download;
mod manager;
mod manifest;
mod migration;
mod package;

pub(crate) use download::download_file;
pub(crate) fn verify_file(path: &std::path::Path, spec: AssetSpec) -> Result<bool, String> {
    package::verify_asset(path, spec, false)
}
pub(crate) fn verify_file_forced(path: &std::path::Path, spec: AssetSpec) -> Result<bool, String> {
    package::verify_asset(path, spec, true)
}
pub(crate) fn verify_file_readonly(
    path: &std::path::Path,
    spec: AssetSpec,
) -> Result<bool, String> {
    package::verify_asset_readonly(path, spec)
}

pub(crate) use manifest::AssetSpec;
pub(super) use manifest::PackageSpec;
pub(super) use manifest::{package_spec, PACKAGES};
pub(super) use migration::prepare_migration;
pub(super) use package::{activate_staging, is_installed, package_dir, staging_dir};

#[cfg(test)]
pub(super) use download::download_into_staging;
pub(crate) fn is_supported(id: &str) -> bool {
    manifest::PACKAGES.iter().any(|spec| spec.id == id)
}
