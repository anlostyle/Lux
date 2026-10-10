use super::*;

const CATALOG_WORKER_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
// Keep the existing per-pool concurrency for Home while isolating it from
// unrelated catalog traffic that could otherwise hold every shared worker.
const MAX_CONCURRENT_HOME_REQUESTS: usize = MAX_CONCURRENT_CATALOG_REQUESTS;

pub(super) fn app_with_state(state: AppState) -> Router {
    let web_root = web_root();
    let resources = state.resources.clone();
    let catalog_request_slots =
        Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_CATALOG_REQUESTS));
    let catalog_workers = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CATALOG_REQUESTS));
    let home_workers = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_HOME_REQUESTS));
    Router::new()
        .route("/logo.svg", get(web_logo))
        .merge(users::api_routes())
        .merge(admin::api_routes())
        .merge(version_priority_api::api_routes())
        .merge(lux_api::api_routes())
        .merge(emby::api_routes())
        .nest("/emby", emby::api_routes())
        .fallback_service(
            ServeDir::new(web_root.clone())
                .append_index_html_on_directories(true)
                .fallback(ServeFile::new(web_root.join("index.html"))),
        )
        .with_state(state)
        .layer(middleware::from_fn(
            move |request: Request<Body>, next: Next| {
                let catalog_request_slots = catalog_request_slots.clone();
                let catalog_workers = catalog_workers.clone();
                let home_workers = home_workers.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let request_headers = request.headers().clone();
                    let request_slot = if is_catalog_aggregation_path(&path) {
                        match catalog_request_slots.try_acquire_owned() {
                            Ok(permit) => Some(permit),
                            Err(_) => return catalog_busy_response(&path, &request_headers),
                        }
                    } else {
                        None
                    };
                    let worker_permit = if request_slot.is_some() {
                        let wait_started = Instant::now();
                        let workers =
                            select_catalog_worker_pool(&path, catalog_workers, home_workers);
                        match acquire_catalog_worker(workers, CATALOG_WORKER_WAIT_TIMEOUT).await {
                            Ok(permit) => Some(permit),
                            Err(CatalogWorkerWaitError::TimedOut) => {
                                tracing::warn!(
                                    request_id = request_headers
                                        .get("x-request-id")
                                        .and_then(|value| value.to_str().ok())
                                        .unwrap_or("unknown"),
                                    route_class = catalog_route_class(&path),
                                    wait_ms = u64::try_from(wait_started.elapsed().as_millis())
                                        .unwrap_or(u64::MAX),
                                    "catalog worker queue wait timed out"
                                );
                                return catalog_busy_response(&path, &request_headers);
                            }
                            Err(CatalogWorkerWaitError::Closed) => {
                                return StatusCode::SERVICE_UNAVAILABLE.into_response();
                            }
                        }
                    } else {
                        None
                    };
                    let response = next.run(request).await;
                    drop(worker_permit);
                    drop(request_slot);
                    response
                }
            },
        ))
        .layer(middleware::from_fn(attach_peer_address))
        .layer(middleware::from_fn(normalize_lux_api_key_query))
        .layer(middleware::from_fn(trace_emby_playback_callback))
        .layer(middleware::from_fn(trace_emby_playback_info))
        .layer(middleware::from_fn(trace_emby_media_stream_failure))
        .layer(middleware::from_fn(reject_unmatched_emby_video_path))
        .layer(middleware::from_fn(reject_unmatched_api_path))
        .layer(middleware::from_fn(normalize_empty_api_service_unavailable))
        .layer(middleware::from_fn(
            move |request: Request<Body>, next: Next| {
                let resources = resources.clone();
                async move {
                    let is_home = is_home_request_path(request.uri().path());
                    let started = Instant::now();
                    let response = next.run(request).await;
                    if is_home {
                        resources.record_home_latency(started.elapsed());
                    }
                    response
                }
            },
        ))
        .layer(
            tower::ServiceBuilder::new()
                .set_x_request_id(MakeRequestUuid)
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &axum::http::Request<_>| {
                            let request_id = request
                                .headers()
                                .get("x-request-id")
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("unknown");
                            tracing::info_span!(
                                "request",
                                method = %request.method(),
                                path = %safe_trace_path(request.uri()),
                                version = ?request.version(),
                                "requestId" = %request_id,
                                "durationMs" = tracing::field::Empty,
                                "statusCode" = tracing::field::Empty,
                                "errorCode" = tracing::field::Empty,
                            )
                        })
                        .on_response(
                            |response: &Response, latency: Duration, span: &tracing::Span| {
                                let duration_ms =
                                    u64::try_from(latency.as_millis()).unwrap_or(u64::MAX);
                                span.record("durationMs", duration_ms);
                                span.record("statusCode", response.status().as_u16());
                                tracing::debug!(
                                    latency = ?latency,
                                    status = %response.status(),
                                    "finished processing request"
                                );
                            },
                        ),
                )
                .propagate_x_request_id(),
        )
}

fn select_catalog_worker_pool(
    path: &str,
    catalog_workers: Arc<tokio::sync::Semaphore>,
    home_workers: Arc<tokio::sync::Semaphore>,
) -> Arc<tokio::sync::Semaphore> {
    if is_home_request_path(path) {
        home_workers
    } else {
        catalog_workers
    }
}

fn is_home_request_path(path: &str) -> bool {
    matches!(path, "/api/v1/home" | "/api/v1/home/carousel")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogWorkerWaitError {
    Closed,
    TimedOut,
}

async fn acquire_catalog_worker(
    workers: Arc<tokio::sync::Semaphore>,
    wait: Duration,
) -> Result<tokio::sync::OwnedSemaphorePermit, CatalogWorkerWaitError> {
    match tokio::time::timeout(wait, workers.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(_)) => Err(CatalogWorkerWaitError::Closed),
        Err(_) => Err(CatalogWorkerWaitError::TimedOut),
    }
}

fn catalog_busy_response(path: &str, headers: &HeaderMap) -> Response {
    let mut response = if path.starts_with("/api/v1/") {
        super::api_error(
            headers,
            StatusCode::SERVICE_UNAVAILABLE,
            lux::ApiErrorCode::CatalogBusy,
            "目录请求繁忙，请稍后重试",
        )
        .into_response()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    };
    response.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        HeaderValue::from_static("2"),
    );
    response
}

fn catalog_route_class(path: &str) -> &'static str {
    if is_home_request_path(path) {
        "home"
    } else if path == "/Items" || path.starts_with("/emby/Items") {
        "emby_items"
    } else {
        "catalog"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homepage_carousel_uses_the_isolated_home_worker_pool() {
        let catalog_workers = Arc::new(tokio::sync::Semaphore::new(1));
        let home_workers = Arc::new(tokio::sync::Semaphore::new(1));

        assert!(Arc::ptr_eq(
            &select_catalog_worker_pool(
                "/api/v1/home/carousel",
                catalog_workers.clone(),
                home_workers.clone(),
            ),
            &home_workers,
        ));
        assert_eq!(catalog_route_class("/api/v1/home/carousel"), "home");
    }

    #[tokio::test]
    async fn catalog_worker_queue_wait_has_a_deadline() {
        let workers = Arc::new(tokio::sync::Semaphore::new(1));
        let held = workers
            .clone()
            .acquire_owned()
            .await
            .expect("worker permit");

        let result = acquire_catalog_worker(workers.clone(), Duration::from_millis(1)).await;
        assert!(matches!(result, Err(CatalogWorkerWaitError::TimedOut)));

        drop(held);
        assert!(
            acquire_catalog_worker(workers, Duration::from_millis(10))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn home_worker_pool_does_not_wait_behind_catalog_workers() {
        let catalog_workers = Arc::new(tokio::sync::Semaphore::new(1));
        let home_workers = Arc::new(tokio::sync::Semaphore::new(1));
        let _held_catalog = catalog_workers
            .clone()
            .acquire_owned()
            .await
            .expect("catalog worker permit");

        let home_pool = select_catalog_worker_pool(
            "/api/v1/home",
            catalog_workers.clone(),
            home_workers.clone(),
        );
        let catalog_pool =
            select_catalog_worker_pool("/api/v1/search", catalog_workers, home_workers);

        assert!(
            acquire_catalog_worker(home_pool, Duration::from_millis(10))
                .await
                .is_ok()
        );
        assert!(matches!(
            acquire_catalog_worker(catalog_pool, Duration::from_millis(1)).await,
            Err(CatalogWorkerWaitError::TimedOut)
        ));
    }

    #[tokio::test]
    async fn catalog_busy_lux_response_has_a_stable_code_and_retry_hint() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-request-id",
            HeaderValue::from_static("catalog-busy-test"),
        );

        let response = catalog_busy_response("/api/v1/home", &headers);

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("2")
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("catalog busy response body");
        let body: Value = serde_json::from_slice(&body).expect("catalog busy response JSON");
        assert_eq!(body["error"]["code"], "CATALOG_BUSY");
        assert_eq!(body["error"]["requestId"], "catalog-busy-test");
    }

    #[tokio::test]
    async fn catalog_busy_emby_response_keeps_its_empty_503_shape() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("emby-busy-test"));

        let response = catalog_busy_response("/Items", &headers);

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("2")
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("Emby busy response body");
        assert!(body.is_empty());
    }
}
