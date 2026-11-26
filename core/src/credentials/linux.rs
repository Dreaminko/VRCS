use std::collections::HashMap;
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use secret_service::blocking::{Item, SecretService};
use secret_service::{EncryptionType, Error};
use zeroize::Zeroizing;

use super::StoredStatus;

const READ_FAILED: &str = "credentials.read_failed";
const WRITE_FAILED: &str = "credentials.write_failed";
const NON_INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(5);
const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(60);

fn attributes(target: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", "VRCS"), ("target", target)])
}

fn is_persistent_item(item: &Item<'_>) -> bool {
    !item
        .item_path
        .as_str()
        .starts_with("/org/freedesktop/secrets/collection/session/")
}

fn is_transport_error(error: &Error) -> bool {
    match error {
        Error::PromptDisconnected => true,
        Error::Zbus(zbus_err) => {
            let detail = zbus_err.to_string();
            !detail.contains("org.freedesktop.Secret.Error.IsLocked")
                && !detail.contains("org.freedesktop.DBus.Error.ServiceUnknown")
                && !detail.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
                && !detail.contains("org.freedesktop.DBus.Error.Spawn.ServiceNotFound")
        }
        _ => false,
    }
}

fn failure(error: Error, operation: &'static str) -> String {
    let detail = error.to_string();
    let is_zbus = matches!(error, Error::Zbus(_));
    let reason = match error {
        Error::Locked => "credentials.locked",
        Error::Prompt => "credentials.cancelled",
        Error::Unavailable => "credentials.no_service",
        _ if detail.contains("org.freedesktop.Secret.Error.IsLocked") => "credentials.locked",
        _ if detail.contains("org.freedesktop.DBus.Error.ServiceUnknown")
            || detail.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
            || detail.contains("org.freedesktop.DBus.Error.Spawn.ServiceNotFound")
            || detail.contains("session bus")
            || detail.contains("DBUS_SESSION_BUS_ADDRESS")
            || detail.contains("Cannot autolaunch D-Bus")
            || (is_zbus && detail.contains("No such file or directory")) =>
        {
            "credentials.no_service"
        }
        _ => operation,
    };
    format!("{reason}: {detail}")
}

enum Request {
    Capability(mpsc::SyncSender<(bool, Option<&'static str>)>),
    Status {
        targets: Vec<String>,
        reply: mpsc::SyncSender<StoredStatus>,
    },
    Read {
        target: String,
        authorize: bool,
        reply: mpsc::SyncSender<Result<Option<String>, String>>,
    },
    Write {
        target: String,
        value: Zeroizing<String>,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    Delete {
        targets: Vec<String>,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
}

#[derive(Default)]
struct KeyringWorker {
    service: Option<SecretService<'static>>,
    cache: HashMap<String, Zeroizing<String>>,
}

impl KeyringWorker {
    fn run_with_service<T>(
        &mut self,
        operation: &'static str,
        mut f: impl FnMut(&SecretService<'static>) -> Result<T, Error>,
    ) -> Result<T, String> {
        for attempt in 0..2 {
            if self.service.is_none() {
                self.service = Some(
                    SecretService::connect(EncryptionType::Dh)
                        .map_err(|err| failure(err, operation))?,
                );
            }
            let service = self.service.as_ref().expect("connected SecretService");
            match f(service) {
                Ok(value) => return Ok(value),
                Err(error) if attempt == 0 && is_transport_error(&error) => {
                    self.service = None;
                }
                Err(error) => return Err(failure(error, operation)),
            }
        }
        unreachable!("loop returns on second attempt")
    }

    fn capability(&mut self) -> (bool, Option<&'static str>) {
        let result = self.run_with_service(READ_FAILED, |service| {
            match service.get_default_collection() {
                Ok(collection) => collection.ensure_unlocked(),
                Err(Error::NoResult) => Ok(()),
                Err(error) => Err(error),
            }
        });
        match result {
            Ok(()) => (true, None),
            Err(error) => (false, super::error_reason_code(&error)),
        }
    }

    fn status(&mut self, targets: &[String]) -> StoredStatus {
        if targets.iter().any(|target| self.cache.contains_key(target)) {
            return StoredStatus {
                configured: true,
                available: true,
                reason_code: None,
            };
        }
        let result = self.run_with_service(READ_FAILED, |service| {
            for target in targets {
                let items = service.search_items(attributes(target))?;
                if items.unlocked.iter().any(is_persistent_item) {
                    return Ok(StoredStatus {
                        configured: true,
                        available: true,
                        reason_code: None,
                    });
                }
                if items.locked.iter().any(is_persistent_item) {
                    return Ok(StoredStatus {
                        configured: true,
                        available: false,
                        reason_code: Some("credentials.locked"),
                    });
                }
            }
            match service.get_default_collection() {
                Ok(collection) => collection.ensure_unlocked()?,
                Err(Error::NoResult) => (),
                Err(error) => return Err(error),
            }
            Ok(StoredStatus {
                configured: false,
                available: true,
                reason_code: Some("credentials.not_set"),
            })
        });
        result.unwrap_or_else(|error| StoredStatus {
            configured: false,
            available: false,
            reason_code: super::error_reason_code(&error),
        })
    }

    fn read(&mut self, target: &str, authorize: bool) -> Result<Option<String>, String> {
        if let Some(cached) = self.cache.get(target) {
            return Ok(Some(cached.as_str().to_owned()));
        }
        let secret = self.run_with_service(READ_FAILED, |service| {
            let mut items = service.search_items(attributes(target))?;
            if !items.unlocked.iter().any(is_persistent_item)
                && !items.locked.iter().any(is_persistent_item)
                && unlock_default_if_locked(service, authorize)?
            {
                items = service.search_items(attributes(target))?;
            }
            let item = if let Some(item) = items.unlocked.into_iter().find(is_persistent_item) {
                item
            } else if let Some(item) = items.locked.into_iter().find(is_persistent_item) {
                if !authorize {
                    return Err(Error::Locked);
                }
                item.unlock()?;
                item
            } else {
                return Ok(None);
            };
            item.ensure_unlocked()?;
            item.get_secret().map(|bytes| Some(Zeroizing::new(bytes)))
        })?;
        let Some(bytes) = secret else {
            return Ok(None);
        };
        let value = std::str::from_utf8(&bytes)
            .map(str::to_owned)
            .map_err(|_| format!("{READ_FAILED}: Secret Service credential is not valid UTF-8"))?;
        self.cache
            .insert(target.to_owned(), Zeroizing::new(value.clone()));
        Ok(Some(value))
    }

    fn write(&mut self, target: &str, value: Zeroizing<String>) -> Result<(), String> {
        self.cache.remove(target);
        self.run_with_service(WRITE_FAILED, |service| {
            let collection = match service.get_default_collection() {
                Ok(collection) => collection,
                Err(Error::NoResult) => service.create_collection("VRCS", "default")?,
                Err(error) => return Err(error),
            };
            if collection.is_locked()? {
                collection.unlock()?;
            }
            collection.ensure_unlocked()?;
            collection.create_item(
                target,
                attributes(target),
                value.as_bytes(),
                true,
                "text/plain; charset=utf-8",
            )?;
            Ok(())
        })?;
        self.cache.insert(target.to_owned(), value);
        Ok(())
    }

    fn delete(&mut self, targets: &[String]) -> Result<(), String> {
        let mut previous_cache = Vec::new();
        for target in targets {
            if let Some(value) = self.cache.remove(target) {
                previous_cache.push((target.clone(), value));
            }
        }
        let mut deleted_items: Vec<(String, Zeroizing<Vec<u8>>)> = Vec::new();
        let result = self.run_with_service(WRITE_FAILED, |service| {
            let mut to_delete = Vec::new();
            for target in targets {
                let items = service.search_items(attributes(target))?;
                let found = items.unlocked.into_iter().chain(items.locked);
                for item in found.filter(is_persistent_item) {
                    to_delete.push((target.clone(), item));
                }
            }
            for (target, item) in to_delete {
                if item.is_locked()? {
                    item.unlock()?;
                }
                item.ensure_unlocked()?;
                let secret = item.get_secret().map(Zeroizing::new)?;
                item.delete()?;
                deleted_items.push((target, secret));
            }
            Ok(())
        });
        if result.is_err() {
            for (target, value) in previous_cache {
                self.cache.insert(target, value);
            }
            if !deleted_items.is_empty() {
                let _ = self.run_with_service(WRITE_FAILED, |service| {
                    let collection = match service.get_default_collection() {
                        Ok(collection) => collection,
                        Err(Error::NoResult) => service.create_collection("VRCS", "default")?,
                        Err(error) => return Err(error),
                    };
                    if collection.is_locked()? {
                        collection.unlock()?;
                    }
                    collection.ensure_unlocked()?;
                    for (target, secret) in &deleted_items {
                        collection.create_item(
                            target,
                            attributes(target),
                            secret.as_slice(),
                            false,
                            "text/plain; charset=utf-8",
                        )?;
                    }
                    Ok(())
                });
            }
        }
        result
    }
}

fn unlock_default_if_locked(service: &SecretService<'_>, authorize: bool) -> Result<bool, Error> {
    let collection = match service.get_default_collection() {
        Ok(collection) => collection,
        Err(Error::NoResult) => return Ok(false),
        Err(error) => return Err(error),
    };
    if !collection.is_locked()? {
        return Ok(false);
    }
    if !authorize {
        return Err(Error::Locked);
    }
    collection.unlock()?;
    collection.ensure_unlocked()?;
    Ok(true)
}

fn worker_sender() -> &'static mpsc::Sender<Request> {
    static SENDER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("vrcs-keyring".into())
            .spawn(move || {
                let mut worker = KeyringWorker::default();
                while let Ok(request) = rx.recv() {
                    match request {
                        Request::Capability(reply) => {
                            let _ = reply.send(worker.capability());
                        }
                        Request::Status { targets, reply } => {
                            let _ = reply.send(worker.status(&targets));
                        }
                        Request::Read {
                            target,
                            authorize,
                            reply,
                        } => {
                            let _ = reply.send(worker.read(&target, authorize));
                        }
                        Request::Write {
                            target,
                            value,
                            reply,
                        } => {
                            let _ = reply.send(worker.write(&target, value));
                        }
                        Request::Delete { targets, reply } => {
                            let _ = reply.send(worker.delete(&targets));
                        }
                    }
                }
            })
            .expect("spawn vrcs-keyring thread");
        tx
    })
}

#[derive(Clone, Copy)]
enum RequestError {
    Timeout,
    Disconnected,
}

impl RequestError {
    fn detail(self) -> &'static str {
        match self {
            Self::Timeout => "Secret Service request timed out",
            Self::Disconnected => "Secret Service worker disconnected",
        }
    }
}

fn send_request<T>(
    timeout: Duration,
    build: impl FnOnce(mpsc::SyncSender<T>) -> Request,
    on_error: impl FnOnce(RequestError) -> T,
) -> T {
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    if worker_sender().send(build(reply_tx)).is_err() {
        tracing::error!("Linux keyring worker disconnected");
        return on_error(RequestError::Disconnected);
    }
    match reply_rx.recv_timeout(timeout) {
        Ok(reply) => reply,
        Err(mpsc::RecvTimeoutError::Timeout) => on_error(RequestError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            tracing::error!("Linux keyring worker disconnected");
            on_error(RequestError::Disconnected)
        }
    }
}

pub(super) fn capability() -> (bool, Option<&'static str>) {
    send_request(NON_INTERACTIVE_TIMEOUT, Request::Capability, |_| {
        (false, Some("credentials.read_failed"))
    })
}

pub(super) fn status(targets: &[String]) -> StoredStatus {
    let owned = targets.to_vec();
    send_request(
        NON_INTERACTIVE_TIMEOUT,
        |reply| Request::Status {
            targets: owned,
            reply,
        },
        |_| StoredStatus {
            configured: false,
            available: false,
            reason_code: Some("credentials.read_failed"),
        },
    )
}

pub(super) fn read(target: &str, authorize: bool) -> Result<Option<String>, String> {
    let timeout = if authorize {
        INTERACTIVE_TIMEOUT
    } else {
        NON_INTERACTIVE_TIMEOUT
    };
    let owned = target.to_owned();
    send_request(
        timeout,
        |reply| Request::Read {
            target: owned,
            authorize,
            reply,
        },
        |error| Err(format!("{READ_FAILED}: {}", error.detail())),
    )
}

pub(super) fn write(target: &str, value: &str) -> Result<(), String> {
    let owned_target = target.to_owned();
    let owned_value = Zeroizing::new(value.to_owned());
    send_request(
        INTERACTIVE_TIMEOUT,
        |reply| Request::Write {
            target: owned_target,
            value: owned_value,
            reply,
        },
        |error| Err(format!("{WRITE_FAILED}: {}", error.detail())),
    )
}

pub(super) fn delete(targets: &[String]) -> Result<(), String> {
    let owned = targets.to_vec();
    send_request(
        INTERACTIVE_TIMEOUT,
        |reply| Request::Delete {
            targets: owned,
            reply,
        },
        |error| Err(format!("{WRITE_FAILED}: {}", error.detail())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_transport_and_lock_errors_accurately() {
        assert!(is_transport_error(&Error::PromptDisconnected));
        assert!(!is_transport_error(&Error::Locked));
        assert!(!is_transport_error(&Error::Prompt));
        assert_eq!(
            failure(Error::Locked, READ_FAILED),
            format!("credentials.locked: {}", Error::Locked)
        );
    }
}
