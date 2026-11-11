use crate::AppState;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

const PORT: u16 = 47653;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
}

struct Running {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

pub struct LocalApi {
    config: Mutex<ApiConfig>,
    running: Mutex<Option<Running>>,
    control: tokio::sync::Mutex<()>,
    events: watch::Sender<Arc<Value>>,
    clients: Arc<Semaphore>,
    path: PathBuf,
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "api_failed")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl LocalApi {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let existing = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .ok()
            .filter(|file| {
                file.metadata()
                    .is_ok_and(|m| m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0)
            })
            .and_then(|file| serde_json::from_reader::<_, ApiConfig>(file).ok())
            .filter(|config| {
                config.port == PORT
                    && config.token.len() == 64
                    && config.token.bytes().all(|b| b.is_ascii_hexdigit())
            });
        let config = match existing {
            Some(config) => config,
            None => ApiConfig {
                enabled: false,
                port: PORT,
                token: random_token()?,
            },
        };
        Ok(Self {
            config: Mutex::new(config),
            running: Mutex::new(None),
            control: tokio::sync::Mutex::new(()),
            events: watch::channel(Arc::new(Value::Null)).0,
            clients: Arc::new(Semaphore::new(8)),
            path,
        })
    }

    pub fn config(&self) -> ApiConfig {
        self.config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn save(&self, config: &ApiConfig) -> Result<(), String> {
        let temporary = self.path.with_extension("tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|_| "api_save_failed")?;
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "api_save_failed")?;
        file.write_all(&serde_json::to_vec(config).map_err(|_| "api_save_failed")?)
            .map_err(|_| "api_save_failed")?;
        std::fs::rename(temporary, &self.path).map_err(|_| "api_save_failed".into())
    }

    pub fn publish(&self, state: &AppState) {
        if self.config().enabled && self.events.receiver_count() > 0 {
            self.events.send_replace(Arc::new(state_payload(state)));
        }
    }

    pub async fn configure(
        state: Arc<AppState>,
        enabled: bool,
        rotate: bool,
    ) -> Result<ApiConfig, String> {
        let api = &state.api;
        let _control = api.control.lock().await;
        let mut config = api.config();
        if rotate && (enabled || config.enabled) {
            return Err("api_disable_first".into());
        }
        if rotate {
            config.token = random_token()?;
        }
        if enabled && api.running.lock().unwrap().is_none() {
            let listener =
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, config.port))
                    .await
                    .map_err(|_| "api_port_busy")?;
            config.port = listener.local_addr().map_err(|_| "api_failed")?.port();
            config.enabled = true;
            api.save(&config)?;
            *api.config.lock().unwrap() = config.clone();
            api.publish(&state);
            let (stop, receiver) = watch::channel(false);
            let app = state.clone();
            let task = tokio::spawn(async move {
                let router = Router::new()
                    .route("/v1/status", get(status))
                    .route("/v1/metrics", get(metrics))
                    .route("/v1/snapshot", get(snapshot))
                    .route("/v1/events", get(events))
                    .fallback(|| async { StatusCode::NOT_FOUND })
                    .layer(axum::middleware::from_fn_with_state(app.clone(), authorize))
                    .with_state(app.clone());
                let mut sampler_stop = receiver.clone();
                let sampler_state = app.clone();
                let sampler = tokio::spawn(async move {
                    let mut timer = tokio::time::interval(Duration::from_secs(10));
                    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tokio::select! {
                            _ = sampler_stop.changed() => break,
                            _ = timer.tick() => {
                                let state = sampler_state.clone();
                                let _ = tauri::async_runtime::spawn_blocking(move || {
                                    crate::worker_priority(true);
                                    let mut monitor = state.monitor.lock().unwrap_or_else(|e| e.into_inner());
                                    if !state.paused.load(std::sync::atomic::Ordering::Relaxed)
                                        && crate::now().saturating_sub(monitor.last.sampled_at) >= 10 {
                                        monitor.sample();
                                    }
                                    drop(monitor);
                                    state.api.publish(&state);
                                }).await;
                            }
                        }
                    }
                });
                let mut server_stop = receiver;
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = server_stop.changed().await;
                    })
                    .await;
                sampler.abort();
            });
            *api.running.lock().unwrap() = Some(Running { stop, task });
        } else {
            config.enabled = enabled;
            api.save(&config)?;
            *api.config.lock().unwrap() = config.clone();
            if !enabled {
                let running = api.running.lock().unwrap().take();
                if let Some(mut running) = running {
                    running.stop.send_replace(true);
                    if tokio::time::timeout(Duration::from_secs(2), &mut running.task)
                        .await
                        .is_err()
                    {
                        running.task.abort();
                    }
                }
            }
        }
        Ok(config)
    }
}

pub fn state_payload(state: &AppState) -> Value {
    let metrics = state
        .monitor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .last
        .clone();
    let mut snapshot = state
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    snapshot.dirty = state.dirty.load(std::sync::atomic::Ordering::Relaxed);
    snapshot.watching = state.watching.load(std::sync::atomic::Ordering::Relaxed);
    json!({"schemaVersion": 1, "version": env!("CARGO_PKG_VERSION"), "observedAt": crate::now(),
        "samplingPaused": state.paused.load(std::sync::atomic::Ordering::Relaxed),
        "busy": state.busy.load(std::sync::atomic::Ordering::Relaxed),
        "metrics": if metrics.sampled_at == 0 { Value::Null } else { json!(metrics) }, "snapshot": snapshot})
}

fn authorized(headers: &HeaderMap, config: &ApiConfig) -> bool {
    config.enabled
        && !headers.contains_key("origin")
        && headers.get("host").and_then(|v| v.to_str().ok())
            == Some(format!("127.0.0.1:{}", config.port).as_str())
        && headers.get("authorization").and_then(|v| v.to_str().ok())
            == Some(format!("Bearer {}", config.token).as_str())
}

async fn authorize(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !authorized(request.headers(), &state.api.config()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}

async fn status(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(
        json!({"schemaVersion": 1, "version": env!("CARGO_PKG_VERSION"), "observedAt": crate::now(),
        "samplingPaused": state.paused.load(std::sync::atomic::Ordering::Relaxed),
        "busy": state.busy.load(std::sync::atomic::Ordering::Relaxed)}),
    )
}
async fn metrics(State(state): State<Arc<AppState>>) -> Json<Value> {
    let metrics = state
        .monitor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .last
        .clone();
    Json(if metrics.sampled_at == 0 {
        Value::Null
    } else {
        json!(metrics)
    })
}
async fn snapshot(State(state): State<Arc<AppState>>) -> Json<Value> {
    let mut snapshot = state
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    snapshot.dirty = state.dirty.load(std::sync::atomic::Ordering::Relaxed);
    snapshot.watching = state.watching.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!(snapshot))
}

async fn events(State(state): State<Arc<AppState>>) -> Response {
    let Ok(permit) = state.api.clients.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let stop = state
        .api
        .running
        .lock()
        .unwrap()
        .as_ref()
        .map(|running| running.stop.subscribe());
    let Some(stop) = stop else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let receiver = state.api.events.subscribe();
    let initial = Some(Arc::new(state_payload(&state)));
    let stream = stream::unfold(
        (receiver, stop, initial, permit),
        |(mut receiver, mut stop, initial, permit)| async move {
            if *stop.borrow() {
                return None;
            }
            let value = if let Some(initial) = initial {
                initial
            } else {
                tokio::select! {
                    _ = stop.changed() => return None,
                    changed = receiver.changed() => { if changed.is_err() { return None; } }
                }
                receiver.borrow_and_update().clone()
            };
            Some((
                Ok::<_, Infallible>(Event::default().event("state").data(value.to_string())),
                (receiver, stop, None, permit),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[tauri::command]
pub fn get_local_api(state: tauri::State<'_, Arc<AppState>>) -> ApiConfig {
    state.api.config()
}

#[tauri::command]
pub async fn set_local_api(
    state: tauri::State<'_, Arc<AppState>>,
    enabled: bool,
    rotate: bool,
) -> Result<ApiConfig, String> {
    LocalApi::configure(state.inner().clone(), enabled, rotate).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_loopback_host_and_bearer_rejects_browser_origins() {
        let config = ApiConfig {
            enabled: true,
            port: PORT,
            token: random_token().unwrap(),
        };
        let mut headers = HeaderMap::new();
        headers.insert("host", format!("127.0.0.1:{PORT}").parse().unwrap());
        assert!(!authorized(&headers, &config));
        headers.insert(
            "authorization",
            format!("Bearer {}", config.token).parse().unwrap(),
        );
        assert!(authorized(&headers, &config));
        headers.insert("origin", "https://example.com".parse().unwrap());
        assert!(!authorized(&headers, &config));
        headers.remove("origin");
        headers.insert("host", "evil.example".parse().unwrap());
        assert!(!authorized(&headers, &config));
        assert_eq!(config.token.len(), 64);
    }

    #[tokio::test]
    async fn serves_cached_data_events_and_stops_without_destructive_routes() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let folder = std::env::temp_dir().join(format!(
            "macspace-api-test-{}-{}",
            std::process::id(),
            random_token().unwrap()
        ));
        std::fs::create_dir(&folder).unwrap();
        let state = Arc::new(AppState {
            monitor: Mutex::new(crate::monitor::Monitor::new()),
            snapshot: Mutex::new(crate::scanner::Snapshot::default()),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            dirty: AtomicBool::new(true),
            store: folder.join("snapshot.json"),
            watcher: Mutex::new(None),
            changed: Mutex::new(Default::default()),
            watching: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            api: LocalApi::new(folder.join("local-api.json")).unwrap(),
        });
        assert!(!state.api.config().enabled);
        state.api.config.lock().unwrap().port = 0;
        let config = LocalApi::configure(state.clone(), true, false)
            .await
            .unwrap();
        let base = format!("http://127.0.0.1:{}", config.port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        assert_eq!(
            client
                .get(format!("{base}/v1/metrics"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(format!("{base}/v1/metrics"))
                .bearer_auth(&config.token)
                .header("Origin", "https://example.org")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = client
            .get(format!("{base}/v1/status"))
            .bearer_auth(&config.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store");
        let status: Value = response.json().await.unwrap();
        assert_eq!(status["samplingPaused"], true);
        assert_eq!(status["schemaVersion"], 1);
        assert!(status.get("token").is_none());
        let metrics: Value = client
            .get(format!("{base}/v1/metrics"))
            .bearer_auth(&config.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(metrics.is_null());
        assert_eq!(
            client
                .post(format!("{base}/v1/snapshot"))
                .bearer_auth(&config.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            client
                .get(format!("{base}/v1/clean"))
                .bearer_auth(&config.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let mut events = client
            .get(format!("{base}/v1/events"))
            .bearer_auth(&config.token)
            .send()
            .await
            .unwrap();
        assert_eq!(events.headers()["content-type"], "text/event-stream");
        let initial = String::from_utf8(events.chunk().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(initial.contains("event: state") && initial.contains("\"samplingPaused\":true"));
        state.dirty.store(false, Ordering::Relaxed);
        state.api.publish(&state);
        let changed = String::from_utf8(events.chunk().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(changed.contains("\"dirty\":false"));
        let stopped = LocalApi::configure(state.clone(), false, false)
            .await
            .unwrap();
        assert!(!stopped.enabled);
        assert!(
            client
                .get(format!("{base}/v1/status"))
                .send()
                .await
                .is_err()
        );
        let rotated = LocalApi::configure(state.clone(), false, true)
            .await
            .unwrap();
        assert_ne!(rotated.token, config.token);
        assert_eq!(
            std::fs::metadata(folder.join("local-api.json"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
