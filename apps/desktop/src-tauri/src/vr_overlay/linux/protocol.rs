//! Prototype-only document and pixel receipts. Production must use the extracted
//! SurfaceHub bridge/state types, not enable this independent sample-state driver.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub generation: u32,
    pub revision: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u8,
    pub op: String,
    pub nonce: Option<String>,
    pub request_id: Option<u64>,
    pub payload: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub key: Key,
    pub regions: Vec<Region>,
    pub page_count: u32,
    pub panels: Option<Vec<Panel>>,
    pub menu_bounds: Option<[f64; 4]>,
    pub unplaced: Option<Vec<u32>>,
    pub capture_bounds: Option<[u32; 4]>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: u32,
    pub bounds: [f64; 4],
    pub circle: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub bounds: [f64; 4],
    pub radius: [f64; 2],
}

impl Layout {
    pub fn valid(&self, key: Key) -> bool {
        self.key == key
            && (1..=4096).contains(&self.page_count)
            && self.regions.len() <= 64
            && self.regions.iter().enumerate().all(|(index, region)| {
                region.id <= 7
                    && valid_bounds(region.bounds)
                    && self.regions[..index]
                        .iter()
                        .all(|other| other.id != region.id)
            })
            && self.menu_bounds.is_none_or(valid_bounds)
            && self.panels.as_ref().is_none_or(|panels| {
                panels.len() <= 64
                    && panels.iter().all(|panel| {
                        valid_bounds(panel.bounds)
                            && panel
                                .radius
                                .iter()
                                .all(|radius| radius.is_finite() && (0.0..=0.5).contains(radius))
                    })
            })
            && self.unplaced.as_ref().is_none_or(|ids| ids.len() <= 64)
            && self.capture_bounds.is_none()
    }

    pub fn hit(&self, point: [f64; 2]) -> Option<u32> {
        self.regions
            .iter()
            .find(|region| {
                let [l, t, r, b] = region.bounds;
                point[0] >= l
                    && point[0] <= r
                    && point[1] >= t
                    && point[1] <= b
                    && (!region.circle.unwrap_or(false)
                        || (((point[0] - (l + r) / 2.) / ((r - l) / 2.)).powi(2)
                            + ((point[1] - (t + b) / 2.) / ((b - t) / 2.)).powi(2))
                            <= 1.)
            })
            .map(|region| region.id)
    }
}

fn valid_bounds([l, t, r, b]: [f64; 4]) -> bool {
    [l, t, r, b]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        && l < r
        && t < b
}

pub struct Session {
    pub nonce: String,
    pub epoch: u32,
    pub key: Key,
    pub layout: Option<(Layout, u32)>,
    pub paint: u32,
    pub submitted: Option<(Key, u32)>,
    pub last_request: u64,
    pub connected: bool,
    pub ready: bool,
    pub fault: Option<String>,
}

impl Session {
    pub fn new(epoch: u32) -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            epoch,
            key: Key {
                generation: epoch,
                revision: 1,
            },
            layout: None,
            paint: 0,
            submitted: None,
            last_request: 0,
            connected: false,
            ready: false,
            fault: None,
        }
    }

    pub fn decode(&mut self, bytes: &str) -> Result<Envelope, &'static str> {
        if bytes.len() > 262144 {
            return Err("request_too_large");
        }
        let envelope: Envelope = serde_json::from_str(bytes).map_err(|_| "invalid_request")?;
        if envelope.version != 1 {
            return Err("invalid_version");
        }
        if envelope.op == "connect" {
            if envelope.nonce.is_some()
                || envelope.request_id.is_some()
                || envelope.payload.is_some()
            {
                return Err("invalid_request");
            }
        } else {
            if envelope.nonce.as_deref() != Some(self.nonce.as_str()) {
                return Err("stale_session");
            }
            let id = envelope
                .request_id
                .filter(|id| *id > self.last_request && *id <= 9007199254740991)
                .ok_or("invalid_request_id")?;
            self.last_request = id;
        }
        Ok(envelope)
    }

    pub fn advance(&mut self) {
        self.key.revision = self
            .key
            .revision
            .checked_add(1)
            .expect("prototype revision exhausted");
        self.layout = None;
        // Keep the old texture for fade retention, but never its input map.
    }

    pub fn current_frame(&self) -> bool {
        self.fault.is_none()
            && self.layout.as_ref().is_some_and(|(layout, serial)| {
                layout.key == self.key
                    && self.submitted.is_some_and(|(key, paint)| {
                        key == self.key && paint == self.paint && at_least(paint, *serial)
                    })
            })
    }

    pub fn accept(&mut self, envelope: &Envelope, state: Value) -> Result<Value, &'static str> {
        let payload = envelope.payload.clone().ok_or("invalid_request")?;
        match envelope.op.as_str() {
            "ready" if payload == json!({}) => {
                self.ready = true;
                Ok(state)
            }
            "layout" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Request {
                    layout: Layout,
                    paint_serial: u32,
                }
                let request: Request =
                    serde_json::from_value(payload).map_err(|_| "invalid_layout")?;
                if state
                    .get("activity")
                    .and_then(|activity| activity.get("active"))
                    .and_then(Value::as_bool)
                    != Some(true)
                    || !request.layout.valid(self.key)
                    || request.paint_serial == 0
                    || self
                        .layout
                        .as_ref()
                        .is_some_and(|(_, serial)| !at_least(request.paint_serial, *serial))
                {
                    return Ok(json!(false));
                }
                self.paint = request.paint_serial;
                self.layout = Some((request.layout, request.paint_serial));
                Ok(json!(true))
            }
            "paint" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Request {
                    key: Key,
                    paint_serial: u32,
                }
                let request: Request =
                    serde_json::from_value(payload).map_err(|_| "invalid_request")?;
                let accepted = state
                    .get("activity")
                    .and_then(|activity| activity.get("active"))
                    .and_then(Value::as_bool)
                    == Some(true)
                    && request.key == self.key
                    && request.paint_serial != 0
                    && (self.paint == 0 || at_least(request.paint_serial, self.paint));
                if accepted {
                    self.paint = request.paint_serial;
                }
                Ok(json!(accepted))
            }
            "snapshot" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Request {
                    key: Option<Key>,
                    activity_request: u32,
                    scroll_top: Option<f64>,
                }
                let request: Request =
                    serde_json::from_value(payload).map_err(|_| "invalid_request")?;
                Ok(json!(
                    request.key == Some(self.key)
                        && state
                            .get("activity")
                            .and_then(|activity| activity.get("request"))
                            .and_then(Value::as_u64)
                            == Some(u64::from(request.activity_request))
                        && request
                            .scroll_top
                            .is_none_or(|top| top.is_finite() && top >= 0.)
                ))
            }
            "fault" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Request {
                    stage: String,
                }
                let request: Request =
                    serde_json::from_value(payload).map_err(|_| "invalid_request")?;
                if !["startup", "render", "unhandled"].contains(&request.stage.as_str()) {
                    return Err("invalid_request");
                }
                self.fault = Some(request.stage);
                self.submitted = None;
                self.layout = None;
                Ok(json!(true))
            }
            _ => Err("invalid_request"),
        }
    }
}

pub fn at_least(value: u32, reference: u32) -> bool {
    super::frame_receipt::serial_at_least(value, reference)
}

/// Decode the existing React same-frame marker; it is outside the submitted crop.
pub fn marker(pixels: &[u8], width: usize, height: usize, scale: usize) -> Option<(Key, u32)> {
    super::frame_receipt::decode_marker_pixels(|bit| {
        let x = ((bit % 64) * 4 + 2) * scale;
        let y = (height + 4 + (bit / 64) * 8) * scale;
        let offset = (y * width + x) * 4;
        pixels.get(offset..offset + 4)?.try_into().ok()
    })
    .map(|(generation, revision, paint)| {
        (
            Key {
                generation,
                revision,
            },
            paint,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_content_and_unsubmitted_paints_revoke_the_hit_map() {
        let mut session = Session::new(2);
        let state = json!({"activity":{"active":true,"request":1}});
        let layout = json!({"key":session.key,"regions":[{"id":6,"bounds":[0.1,0.1,0.3,0.3],"circle":true}],"page_count":1});
        let envelope = |op: &str, payload| Envelope {
            version: 1,
            op: op.into(),
            nonce: Some("test".into()),
            request_id: Some(1),
            payload: Some(payload),
        };
        assert_eq!(
            session
                .accept(
                    &envelope("layout", json!({"layout":layout,"paint_serial":3})),
                    state.clone()
                )
                .unwrap(),
            json!(true)
        );
        assert!(!session.current_frame());
        session.submitted = Some((session.key, 3));
        assert!(session.current_frame());
        session
            .accept(
                &envelope("paint", json!({"key":session.key,"paint_serial":4})),
                state,
            )
            .unwrap();
        assert!(!session.current_frame());
        session.submitted = Some((session.key, 4));
        assert!(session.current_frame());
        session.advance();
        assert!(!session.current_frame());
    }

    #[test]
    fn stale_nonce_and_replayed_requests_cannot_mutate_the_document() {
        let mut session = Session::new(1);
        let call = |nonce: &str| {
            json!({"version":1,"op":"ready","nonce":nonce,"request_id":1,"payload":{}}).to_string()
        };
        assert_eq!(
            session.decode(&call("old-document")).unwrap_err(),
            "stale_session"
        );
        let nonce = session.nonce.clone();
        assert!(session.decode(&call(&nonce)).is_ok());
        assert_eq!(
            session.decode(&call(&nonce)).unwrap_err(),
            "invalid_request_id"
        );
    }
}
