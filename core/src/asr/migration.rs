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
    if paths_equivalent(&source, &destination) {
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

pub(crate) fn paths_equivalent(left: &std::path::Path, right: &std::path::Path) -> bool {
    let equivalent = |left: &std::path::Path, right: &std::path::Path| {
        #[cfg(windows)]
        {
            let normalize = |path: &std::path::Path| {
                path.to_string_lossy()
                    .replace('/', "\\")
                    .trim_end_matches('\\')
                    .to_lowercase()
            };
            normalize(left) == normalize(right)
        }
        #[cfg(not(windows))]
        {
            left == right
        }
    };
    equivalent(left, right)
        || matches!(
            (
                std::fs::canonicalize(left),
                std::fs::canonicalize(right)
            ),
            (Ok(left), Ok(right)) if equivalent(&left, &right)
        )
}
