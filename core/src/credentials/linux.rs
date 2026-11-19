use std::collections::HashMap;

use secret_service::blocking::{Item, SecretService};
use secret_service::{EncryptionType, Error};

use super::StoredStatus;

const READ_FAILED: &str = "credentials.read_failed";
const WRITE_FAILED: &str = "credentials.write_failed";

fn attributes(target: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", "VRCS"), ("target", target)])
}

fn failure(error: Error, operation: &'static str) -> String {
    let detail = error.to_string();
    let reason = match error {
        Error::Locked => "credentials.locked",
        Error::Prompt => "credentials.cancelled",
        Error::Unavailable => "credentials.no_service",
        _ if detail.contains("org.freedesktop.Secret.Error.IsLocked") => "credentials.locked",
        _ if detail.contains("org.freedesktop.DBus.Error.ServiceUnknown")
            || detail.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
            || detail.contains("org.freedesktop.DBus.Error.Spawn.ServiceNotFound") =>
        {
            "credentials.no_service"
        }
        _ => operation,
    };
    format!("{reason}: {detail}")
}

fn outside_runtime<T: Send>(operation: impl FnOnce() -> T + Send) -> T {
    if tokio::runtime::Handle::try_current().is_ok() {
        // Cargo feature unification can enable zbus's Tokio backend through
        // desktop dependencies. Keep connection creation, calls and drop outside
        // the caller's runtime; moving only connect would still nest block_on.
        std::thread::scope(|scope| match scope.spawn(operation).join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        })
    } else {
        operation()
    }
}

pub(super) fn capability() -> (bool, Option<&'static str>) {
    outside_runtime(capability_blocking)
}

pub(super) fn status(targets: &[String]) -> StoredStatus {
    outside_runtime(|| status_blocking(targets))
}

pub(super) fn read(target: &str, authorize: bool) -> Result<Option<String>, String> {
    outside_runtime(|| read_blocking(target, authorize))
}

pub(super) fn write(target: &str, value: &str) -> Result<(), String> {
    outside_runtime(|| write_blocking(target, value))
}

pub(super) fn delete(targets: &[String]) -> Result<(), String> {
    outside_runtime(|| delete_blocking(targets))
}

fn connect<'a>(operation: &'static str) -> Result<SecretService<'a>, String> {
    SecretService::connect(EncryptionType::Dh).map_err(|error| failure(error, operation))
}

fn capability_blocking() -> (bool, Option<&'static str>) {
    let result = (|| {
        let service = connect(READ_FAILED)?;
        let result = match service.get_default_collection() {
            Ok(collection) => collection
                .ensure_unlocked()
                .map_err(|error| failure(error, READ_FAILED)),
            // An explicit save can create a persistent default collection. Never use the
            // session collection, whose secrets would disappear after logout.
            Err(Error::NoResult) => Ok(()),
            Err(error) => Err(failure(error, READ_FAILED)),
        };
        result
    })();
    match result {
        Ok(()) => (true, None),
        Err(error) => (false, super::error_reason_code(&error)),
    }
}

fn status_blocking(targets: &[String]) -> StoredStatus {
    let result = (|| {
        let service = connect(READ_FAILED)?;
        for target in targets {
            let items = service
                .search_items(attributes(target))
                .map_err(|error| failure(error, READ_FAILED))?;
            if !items.unlocked.is_empty() {
                return Ok(StoredStatus {
                    configured: true,
                    available: true,
                    reason_code: None,
                });
            }
            if !items.locked.is_empty() {
                return Ok(StoredStatus {
                    configured: true,
                    available: false,
                    reason_code: Some("credentials.locked"),
                });
            }
        }
        match service.get_default_collection() {
            Ok(collection) => collection
                .ensure_unlocked()
                .map_err(|error| failure(error, READ_FAILED))?,
            Err(Error::NoResult) => (),
            Err(error) => return Err(failure(error, READ_FAILED)),
        }
        Ok(StoredStatus {
            configured: false,
            available: true,
            reason_code: Some("credentials.not_set"),
        })
    })();
    result.unwrap_or_else(|error| StoredStatus {
        configured: false,
        available: false,
        reason_code: super::error_reason_code(&error),
    })
}

fn read_blocking(target: &str, authorize: bool) -> Result<Option<String>, String> {
    let service = connect(READ_FAILED)?;
    let items = service
        .search_items(attributes(target))
        .map_err(|error| failure(error, READ_FAILED))?;
    let item = if let Some(item) = items.unlocked.into_iter().next() {
        item
    } else if let Some(item) = items.locked.into_iter().next() {
        if !authorize {
            return Err("credentials.locked: Unlock the keyring through an explicit save or use action".into());
        }
        item.unlock().map_err(|error| failure(error, READ_FAILED))?;
        item
    } else {
        // Searching does not unlock or prompt. A missing entry is distinct from an
        // unavailable service, even when no collection has been created yet.
        return Ok(None);
    };
    item.ensure_unlocked()
        .map_err(|error| failure(error, READ_FAILED))?;
    let bytes = item.get_secret().map_err(|error| failure(error, READ_FAILED))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "credentials.read_failed: Stored credential is not valid UTF-8".into())
}

fn write_blocking(target: &str, value: &str) -> Result<(), String> {
    let service = connect(WRITE_FAILED)?;
    let items = service
        .search_items(attributes(target))
        .map_err(|error| failure(error, WRITE_FAILED))?;
    if !items.unlocked.is_empty() || !items.locked.is_empty() {
        let items: Vec<_> = items.unlocked.into_iter().chain(items.locked).collect();
        authorize_items(&items)?;
        let previous = items
            .iter()
            .map(|item| {
                Ok((
                    item.get_secret().map_err(|error| failure(error, WRITE_FAILED))?,
                    item.get_secret_content_type().map_err(|error| failure(error, WRITE_FAILED))?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Secret Service has no transaction or compare-and-swap. Restore every
        // attempted write on failure, including an ambiguously acknowledged call.
        // Another process can still change or lock these items during recovery.
        for (index, item) in items.iter().enumerate() {
            if let Err(error) = item.set_secret(value.as_bytes(), "text/plain; charset=utf-8") {
                let recovery = items[..=index]
                    .iter()
                    .zip(&previous[..=index])
                    .filter_map(|(item, (secret, content_type))| {
                        item.set_secret(secret, content_type).err().map(|error| failure(error, WRITE_FAILED))
                    })
                    .collect::<Vec<_>>();
                return Err(mutation_failure(error, recovery));
            }
        }
        return Ok(());
    }
    let collection = match service.get_default_collection() {
        Ok(collection) => collection,
        Err(Error::NoResult) => service
            .create_collection("VRCS", "default")
            .map_err(|error| failure(error, WRITE_FAILED))?,
        Err(error) => return Err(failure(error, WRITE_FAILED)),
    };
    if collection.is_locked().map_err(|error| failure(error, WRITE_FAILED))? {
        collection.unlock().map_err(|error| failure(error, WRITE_FAILED))?;
    }
    collection.ensure_unlocked()
        .map_err(|error| failure(error, WRITE_FAILED))?;
    collection
        .create_item(target, attributes(target), value.as_bytes(), true, "text/plain; charset=utf-8")
        .map_err(|error| failure(error, WRITE_FAILED))?;
    Ok(())
}

fn authorize_items(items: &[Item<'_>]) -> Result<(), String> {
    // Finish all unlock prompts before changing even an already unlocked item.
    for item in items {
        if item.is_locked().map_err(|error| failure(error, WRITE_FAILED))? {
            item.unlock().map_err(|error| failure(error, WRITE_FAILED))?;
        }
        item.ensure_unlocked().map_err(|error| failure(error, WRITE_FAILED))?;
    }
    Ok(())
}

fn mutation_failure(error: Error, recovery: Vec<String>) -> String {
    let error = failure(error, WRITE_FAILED);
    if recovery.is_empty() {
        error
    } else {
        format!("credentials.rollback_failed: {error}; credential recovery failed: {}", recovery.join("; "))
    }
}

fn delete_blocking(targets: &[String]) -> Result<(), String> {
    let service = connect(WRITE_FAILED)?;
    let mut items = Vec::new();
    for target in targets {
        let matches = service.search_items(attributes(target)).map_err(|error| failure(error, WRITE_FAILED))?;
        items.extend(matches.unlocked);
        items.extend(matches.locked);
    }
    if items.is_empty() {
        return Ok(());
    }
    authorize_items(&items)?;
    let collections = service.get_all_collections().map_err(|error| failure(error, WRITE_FAILED))?;
    // Discover collection membership through the API; item paths need not be
    // children of their collection's object path.
    let mut membership = HashMap::new();
    for (index, collection) in collections.iter().enumerate() {
        for target in targets {
            for item in collection.search_items(attributes(target)).map_err(|error| failure(error, WRITE_FAILED))? {
                membership.insert(item.item_path, index);
            }
        }
    }
    let previous = items.iter().map(|item| {
        let collection = *membership.get(&item.item_path).ok_or_else(|| {
            "credentials.write_failed: Cannot locate the credential's collection for recovery".to_string()
        })?;
        collections[collection].ensure_unlocked().map_err(|error| failure(error, WRITE_FAILED))?;
        Ok((
            collection,
            item.get_label().map_err(|error| failure(error, WRITE_FAILED))?,
            item.get_attributes().map_err(|error| failure(error, WRITE_FAILED))?,
            item.get_secret().map_err(|error| failure(error, WRITE_FAILED))?,
            item.get_secret_content_type().map_err(|error| failure(error, WRITE_FAILED))?,
        ))
    }).collect::<Result<Vec<_>, String>>()?;
    // Delete itself can prompt; the API cannot preauthorize all deletes. On any
    // failure (including cancellation) recreate removed items in their original
    // collections, preserving secrets, labels, attributes and content types.
    // Object paths/timestamps and cross-process atomicity cannot be preserved.
    for (index, item) in items.iter().enumerate() {
        if let Err(error) = item.delete() {
            let mut recovery = Vec::new();
            for (item, (collection, label, attributes, secret, content_type)) in
                items[..=index].iter().zip(&previous[..=index])
            {
                let collection = &collections[*collection];
                let search_attributes = attributes.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
                let restore = (|| {
                    // Check the failed call too: a lost reply may hide a deletion.
                    if !collection.search_items(search_attributes)?.iter().any(|found| found.item_path == item.item_path) {
                        let restore_attributes = attributes.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
                        collection.create_item(label, restore_attributes, secret, false, content_type)?;
                    }
                    Ok::<_, Error>(())
                })();
                if let Err(error) = restore {
                    recovery.push(failure(error, WRITE_FAILED));
                }
            }
            return Err(mutation_failure(error, recovery));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a dedicated user D-Bus session and an unlocked disposable Secret Service keyring"]
    fn isolated_keyring_preserves_profile_lifecycle() {
        assert_eq!(
            std::env::var("VRCS_SECRET_SERVICE_TEST").as_deref(),
            Ok("isolated"),
            "Run only in a disposable account/keyring with VRCS_SECRET_SERVICE_TEST=isolated",
        );
        let first = format!("VRCS/API/profile/test-{}", uuid::Uuid::new_v4());
        let second = format!("VRCS/API/profile/test-{}", uuid::Uuid::new_v4());
        struct Cleanup<'a>([&'a str; 2]);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                for target in self.0 {
                    let _ = delete(&[target.to_owned()]);
                }
            }
        }
        let _cleanup = Cleanup([&first, &second]);

        assert_eq!(read(&first, false).unwrap(), None);
        assert_eq!(status(std::slice::from_ref(&first)).reason_code, Some("credentials.not_set"));
        write(&first, "first-profile-secret").unwrap();
        write(&second, "second-profile-secret").unwrap();
        // Every operation creates a new D-Bus connection; there is no in-process
        // secret cache that could hide a failed persistent read after reconnect.
        assert_eq!(read(&first, false).unwrap().as_deref(), Some("first-profile-secret"));
        assert_eq!(read(&second, false).unwrap().as_deref(), Some("second-profile-secret"));
        assert!(status(std::slice::from_ref(&first)).configured);
        let service = connect(WRITE_FAILED).unwrap();
        let collection = service.get_default_collection().unwrap();
        collection.create_item("duplicate profile", attributes(&first), b"stale-secret", false, "text/plain").unwrap();
        write(&first, "updated-profile-secret").unwrap();
        let duplicates = service.search_items(attributes(&first)).unwrap().unlocked;
        assert_eq!(duplicates.len(), 2);
        assert!(duplicates.iter().all(|item| item.get_secret().unwrap().as_slice() == b"updated-profile-secret"));
        assert_eq!(read(&first, false).unwrap().as_deref(), Some("updated-profile-secret"));
        assert_eq!(read(&second, false).unwrap().as_deref(), Some("second-profile-secret"));
        delete(&[first.clone()]).unwrap();
        delete(&[first.clone()]).unwrap();
        assert_eq!(read(&first, false).unwrap(), None);
        assert_eq!(read(&second, false).unwrap().as_deref(), Some("second-profile-secret"));
        delete(&[second.clone()]).unwrap();
        assert_eq!(read(&second, false).unwrap(), None);
    }
}
