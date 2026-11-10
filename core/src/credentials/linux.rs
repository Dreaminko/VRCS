use std::collections::HashMap;

use secret_service::blocking::SecretService;
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

pub(super) fn delete(target: &str) -> Result<(), String> {
    outside_runtime(|| delete_blocking(target))
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
        // Update every exact match so an old duplicate cannot reappear on restart.
        for item in items.unlocked.iter().chain(&items.locked) {
            if item.is_locked().map_err(|error| failure(error, WRITE_FAILED))? {
                item.unlock().map_err(|error| failure(error, WRITE_FAILED))?;
            }
            item.ensure_unlocked()
                .map_err(|error| failure(error, WRITE_FAILED))?;
            item.set_secret(value.as_bytes(), "text/plain; charset=utf-8")
                .map_err(|error| failure(error, WRITE_FAILED))?;
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

fn delete_blocking(target: &str) -> Result<(), String> {
    let service = connect(WRITE_FAILED)?;
    let items = service
        .search_items(attributes(target))
        .map_err(|error| failure(error, WRITE_FAILED))?;
    for item in items.unlocked.iter().chain(&items.locked) {
        if item.is_locked().map_err(|error| failure(error, WRITE_FAILED))? {
            item.unlock().map_err(|error| failure(error, WRITE_FAILED))?;
        }
        item.delete().map_err(|error| failure(error, WRITE_FAILED))?;
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
                    let _ = delete(target);
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
        write(&first, "updated-profile-secret").unwrap();
        assert_eq!(read(&first, false).unwrap().as_deref(), Some("updated-profile-secret"));
        assert_eq!(read(&second, false).unwrap().as_deref(), Some("second-profile-secret"));
        delete(&first).unwrap();
        delete(&first).unwrap();
        assert_eq!(read(&first, false).unwrap(), None);
        assert_eq!(read(&second, false).unwrap().as_deref(), Some("second-profile-secret"));
        delete(&second).unwrap();
        assert_eq!(read(&second, false).unwrap(), None);
    }
}
