use std::path::PathBuf;

use super::qwen_models::{prepare_migration, PackageSpec, PACKAGES};

pub(super) fn move_model_dir(source: PathBuf, destination: PathBuf) -> Result<(), String> {
    move_model_dir_with_specs(source, destination, &PACKAGES)
}

pub(super) fn move_model_dir_with_specs(
    source: PathBuf,
    destination: PathBuf,
    specs: &[PackageSpec],
) -> Result<(), String> {
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    if source == destination
        || matches!((std::fs::canonicalize(&source), std::fs::canonicalize(&destination)),
            (Ok(source), Ok(destination)) if source == destination)
    {
        return Ok(());
    }
    let migration = prepare_migration(&source, &destination, specs)?;
    // Valid destination packages are already available; a cleanup failure must
    // not invalidate the new storage root or delete its verified copy.
    if let Err(error) = migration.commit() {
        tracing::warn!(%error, "Qwen ASR package moved, but old copy could not be removed");
    }
    Ok(())
}
