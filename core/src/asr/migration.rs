use std::path::PathBuf;

use super::model::{
    file_sha256, record_verification, verification_path, verify_model_file, MODELS,
};

pub(super) fn move_model_dir(source_dir: PathBuf, model_dir: PathBuf) -> Result<(), String> {
    #[derive(Clone, Copy)]
    enum TransferKind {
        Renamed,
        Copied,
        AlreadyPresent,
    }

    struct Transfer {
        source: PathBuf,
        destination: PathBuf,
        kind: TransferKind,
    }

    fn rollback(transfers: &[Transfer]) {
        for transfer in transfers.iter().rev() {
            match transfer.kind {
                TransferKind::Renamed => {
                    let _ = std::fs::rename(&transfer.destination, &transfer.source);
                    let _ = std::fs::remove_file(verification_path(&transfer.destination));
                }
                TransferKind::Copied => {
                    let _ = std::fs::remove_file(&transfer.destination);
                    let _ = std::fs::remove_file(verification_path(&transfer.destination));
                }
                TransferKind::AlreadyPresent => {}
            }
        }
    }

    std::fs::create_dir_all(&model_dir)
        .map_err(|error| format!("无法创建 ASR 模型目录 {}：{error}", model_dir.display()))?;
    let same_directory = source_dir == model_dir
        || matches!(
            (
                std::fs::canonicalize(&source_dir),
                std::fs::canonicalize(&model_dir)
            ),
            (Ok(source), Ok(destination)) if source == destination
        );
    if same_directory {
        return Ok(());
    }

    let models = MODELS
        .iter()
        .filter_map(|spec| {
            let source = source_dir.join(spec.filename);
            source
                .metadata()
                .ok()
                .filter(|metadata| metadata.is_file() && metadata.len() == spec.expected_bytes)
                .map(|_| (*spec, source, model_dir.join(spec.filename)))
        })
        .collect::<Vec<_>>();
    for (spec, source, destination) in &models {
        if !verify_model_file(source, *spec, false)? {
            return Err(format!(
                "源目录中的模型文件 {} 校验失败，请重新下载",
                source.display()
            ));
        }
        if let Ok(metadata) = destination.metadata() {
            if !metadata.is_file() || metadata.len() != spec.expected_bytes {
                return Err(format!(
                    "目标目录中已存在不完整的模型文件 {}，请移走后重试",
                    destination.display()
                ));
            }
            if !verify_model_file(destination, *spec, false)? {
                return Err(format!(
                    "目标目录中的模型文件 {} 校验失败，请移走后重试",
                    destination.display()
                ));
            }
        }
    }

    let mut transfers = Vec::with_capacity(models.len());
    for (spec, source, destination) in models {
        if destination.exists() {
            transfers.push(Transfer {
                source,
                destination,
                kind: TransferKind::AlreadyPresent,
            });
            continue;
        }
        match std::fs::rename(&source, &destination) {
            Ok(()) => {
                if let Err(error) = record_verification(&destination, spec, spec.sha256) {
                    tracing::warn!(model = spec.id, %error, "unable to cache migrated model verification");
                }
                transfers.push(Transfer {
                    source,
                    destination,
                    kind: TransferKind::Renamed,
                });
            }
            Err(_) => {
                let temporary =
                    destination.with_extension(format!("moving-{}", std::process::id()));
                if temporary.exists() {
                    rollback(&transfers);
                    return Err(format!(
                        "目标目录中存在未完成的迁移文件 {}，请移走后重试",
                        temporary.display()
                    ));
                }
                let copied = match std::fs::copy(&source, &temporary) {
                    Ok(copied) => copied,
                    Err(error) => {
                        let _ = std::fs::remove_file(&temporary);
                        rollback(&transfers);
                        return Err(format!("无法移动模型文件 {}：{error}", source.display()));
                    }
                };
                if copied != spec.expected_bytes {
                    let _ = std::fs::remove_file(&temporary);
                    rollback(&transfers);
                    return Err(format!(
                        "模型文件复制不完整：应为 {} 字节，实际为 {copied} 字节",
                        spec.expected_bytes
                    ));
                }
                let copied_digest = match file_sha256(&temporary) {
                    Ok(digest) => digest,
                    Err(error) => {
                        let _ = std::fs::remove_file(&temporary);
                        rollback(&transfers);
                        return Err(error);
                    }
                };
                if copied_digest != spec.sha256 {
                    let _ = std::fs::remove_file(&temporary);
                    rollback(&transfers);
                    return Err(format!("模型文件复制校验失败：{}", source.display()));
                }
                if let Err(error) = std::fs::rename(&temporary, &destination) {
                    let _ = std::fs::remove_file(&temporary);
                    rollback(&transfers);
                    return Err(format!(
                        "无法完成模型文件迁移 {}：{error}",
                        destination.display()
                    ));
                }
                if let Err(error) = record_verification(&destination, spec, &copied_digest) {
                    tracing::warn!(model = spec.id, %error, "unable to cache migrated model verification");
                }
                transfers.push(Transfer {
                    source,
                    destination,
                    kind: TransferKind::Copied,
                });
            }
        }
    }

    for transfer in &transfers {
        if matches!(
            transfer.kind,
            TransferKind::Copied | TransferKind::AlreadyPresent
        ) {
            if let Err(error) = std::fs::remove_file(&transfer.source) {
                tracing::warn!(
                    path = %transfer.source.display(),
                    %error,
                    "模型已迁移，但无法删除旧目录中的副本"
                );
            }
        }
        let _ = std::fs::remove_file(verification_path(&transfer.source));
    }
    let _ = std::fs::remove_dir(&source_dir);
    Ok(())
}
