use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::RawQuery;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::SinkExt;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const SERVICE_TYPE: &str = "_oscjson._tcp.local.";

#[derive(Clone, Copy)]
pub(crate) enum OscQueryConsumer {
    MuteSync = 0,
    UniText = 1,
}

#[derive(Clone)]
pub(crate) struct OscQueryDiscovery {
    consumers: Arc<Mutex<[bool; 2]>>,
    active_tx: watch::Sender<bool>,
    endpoint_tx: watch::Sender<Option<u16>>,
    rescan: Arc<Notify>,
}

impl OscQueryDiscovery {
    pub(crate) fn new(shutdown: watch::Receiver<bool>) -> Self {
        let consumers = Arc::new(Mutex::new([false; 2]));
        let (active_tx, active_rx) = watch::channel(false);
        let (endpoint_tx, _) = watch::channel(None);
        let rescan = Arc::new(Notify::new());
        tokio::spawn(run_discovery(
            active_rx,
            shutdown,
            endpoint_tx.clone(),
            Arc::clone(&rescan),
        ));
        Self {
            consumers,
            active_tx,
            endpoint_tx,
            rescan,
        }
    }

    pub(crate) fn set_enabled(&self, consumer: OscQueryConsumer, enabled: bool) {
        let any_enabled = {
            let mut guard = self.consumers.lock().expect("OSCQuery consumer lock");
            guard[consumer as usize] = enabled;
            guard[0] || guard[1]
        };
        self.active_tx.send_if_modified(|current| {
            if *current == any_enabled {
                false
            } else {
                *current = any_enabled;
                true
            }
        });
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<u16>> {
        self.endpoint_tx.subscribe()
    }

    pub(crate) fn report_unavailable(&self, port: u16) {
        let cleared = self.endpoint_tx.send_if_modified(|current| {
            if *current == Some(port) {
                *current = None;
                true
            } else {
                false
            }
        });
        if cleared {
            self.rescan.notify_one();
        }
    }
}

async fn run_discovery(
    mut active_rx: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    endpoint_tx: watch::Sender<Option<u16>>,
    rescan: Arc<Notify>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        if !*active_rx.borrow() {
            endpoint_tx.send_if_modified(|current| {
                if current.is_some() {
                    *current = None;
                    true
                } else {
                    false
                }
            });
            tokio::select! {
                _ = shutdown.changed() => continue,
                _ = active_rx.changed() => continue,
            }
        }

        match browse_vrchat_port(&mut active_rx, &mut shutdown).await {
            Ok(Some(port)) => {
                endpoint_tx.send_replace(Some(port));
                tokio::select! {
                    _ = shutdown.changed() => continue,
                    _ = active_rx.changed() => continue,
                    _ = rescan.notified() => {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                            _ = shutdown.changed() => {},
                            _ = active_rx.changed() => {},
                        }
                    }
                }
            }
            Ok(None) => continue,
            Err(error) => {
                tracing::debug!(%error, "VRChat OSCQuery mDNS browse failed");
                endpoint_tx.send_replace(None);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                    _ = shutdown.changed() => {},
                    _ = active_rx.changed() => {},
                }
            }
        }
    }
}

async fn browse_vrchat_port(
    active_rx: &mut watch::Receiver<bool>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Option<u16>, String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let services = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    let port = loop {
        if !*active_rx.borrow() || *shutdown.borrow() {
            let _ = daemon.shutdown();
            return Ok(None);
        }
        tokio::select! {
            event = services.recv_async() => match event.map_err(|error| error.to_string())? {
                ServiceEvent::ServiceResolved(info)
                    if info.get_fullname().to_ascii_lowercase().contains("vrchat-client")
                        && info.get_port() != 0
                        && info.get_addresses().iter().any(|addr| is_local_ip(addr.to_ip_addr())) =>
                {
                    break info.get_port();
                }
                _ => continue,
            },
            _ = active_rx.changed() => {
                let _ = daemon.shutdown();
                return Ok(None);
            }
            _ = shutdown.changed() => {
                let _ = daemon.shutdown();
                return Ok(None);
            }
        }
    };
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(Some(port))
}

fn is_local_ip(ip: IpAddr) -> bool {
    ip.is_loopback() || std::net::UdpSocket::bind(SocketAddr::new(ip, 0)).is_ok()
}

pub(crate) type OscQueryWs =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(crate) async fn connect_ws_and_listen(port: u16, paths: &[&str]) -> Result<OscQueryWs, String> {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .map_err(|error| error.to_string())?;
    for path in paths {
        ws.send(Message::Text(
            json!({ "COMMAND": "LISTEN", "DATA": path })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(ws)
}

pub(crate) struct OscQueryAdvertisement {
    daemon: ServiceDaemon,
    fullname: String,
    task: JoinHandle<()>,
}

impl OscQueryAdvertisement {
    pub(crate) async fn start(osc_udp_port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| error.to_string())?;
        let http_port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let router = Router::new()
            .route(
                "/",
                get(move |RawQuery(query): RawQuery| async move {
                    let wants_host_info = query.as_deref().is_some_and(|raw| {
                        raw.split('&')
                            .any(|part| part.split('=').next() == Some("HOST_INFO"))
                    });
                    Json(if wants_host_info {
                        host_info_json(osc_udp_port)
                    } else {
                        root_node_json()
                    })
                }),
            )
            .route("/avatar", get(|| async { Json(avatar_node_json()) }));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            "VRCS-UniText",
            "VRCS-UniText.local.",
            "127.0.0.1",
            http_port,
            None::<HashMap<String, String>>,
        )
        .map_err(|error| error.to_string())?;
        let fullname = info.get_fullname().to_string();
        if let Err(error) = daemon.register(info) {
            task.abort();
            let _ = daemon.shutdown();
            return Err(error.to_string());
        }
        Ok(Self {
            daemon,
            fullname,
            task,
        })
    }
}

impl Drop for OscQueryAdvertisement {
    fn drop(&mut self) {
        self.task.abort();
        if let Ok(receiver) = self.daemon.unregister(&self.fullname) {
            let _ = receiver.recv_timeout(Duration::from_millis(100));
        }
        let _ = self.daemon.shutdown();
    }
}

fn host_info_json(osc_udp_port: u16) -> Value {
    json!({
        "NAME": "VRCS-UniText",
        "OSC_IP": "127.0.0.1",
        "OSC_PORT": osc_udp_port,
        "OSC_TRANSPORT": "UDP",
        "EXTENSIONS": { "ACCESS": true, "VALUE": true },
    })
}

fn avatar_node_json() -> Value {
    json!({
        "FULL_PATH": "/avatar",
        "ACCESS": 1,
    })
}

fn root_node_json() -> Value {
    json!({
        "DESCRIPTION": "root",
        "FULL_PATH": "/",
        "ACCESS": 0,
        "CONTENTS": {
            "avatar": avatar_node_json(),
        }
    })
}
