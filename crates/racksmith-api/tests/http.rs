//! End-to-end tests of the HTTP API, driving the router in memory with `tower::ServiceExt`.
//!
//! Every endpoint and every documented status code is exercised. No sockets are opened.

// Dependencies of the library that these tests do not use directly.
use racksmith_domain as _;
use serde as _;
use thiserror as _;
use tracing as _;

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use http_body_util::BodyExt as _;
    use racksmith_api::{AppState, router};
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    const MAC: &str = "52:54:00:12:34:56";
    const FIRST_ID: &str = "m-0000000000000001";
    const UNKNOWN_ID: &str = "m-00000000000000ff";

    /// A response reduced to what the tests assert on.
    struct TestResponse {
        status: StatusCode,
        location: Option<String>,
        body: Value,
    }

    fn app() -> Router {
        router(AppState::default())
    }

    /// Sends a request with an optional raw JSON body and decodes the JSON response.
    async fn send(app: &Router, method: Method, uri: &str, json: Option<String>) -> TestResponse {
        let builder = Request::builder().method(method).uri(uri);
        let request = match json {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json)),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let location = response
            .headers()
            .get(header::LOCATION)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!("response body is not JSON ({error}): {bytes:?}");
        });
        TestResponse {
            status,
            location,
            body,
        }
    }

    async fn get(app: &Router, uri: &str) -> TestResponse {
        send(app, Method::GET, uri, None).await
    }

    async fn post(app: &Router, uri: &str, body: Value) -> TestResponse {
        send(app, Method::POST, uri, Some(body.to_string())).await
    }

    async fn create(app: &Router, body: Value) -> TestResponse {
        post(app, "/api/v1/machines", body).await
    }

    async fn apply(app: &Router, id: &str, event: &str) -> TestResponse {
        post(
            app,
            &format!("/api/v1/machines/{id}/events"),
            json!({ "event": event }),
        )
        .await
    }

    /// Asserts that `response` is an error with the given status and code.
    fn assert_error(response: &TestResponse, status: StatusCode, code: &str) {
        assert_eq!(response.status, status, "status of {}", response.body);
        assert_eq!(
            response.body["error"]["code"], code,
            "code of {}",
            response.body
        );
        assert!(
            response.body["error"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "error has a message: {}",
            response.body
        );
    }

    #[tokio::test]
    async fn healthz_reports_ok() {
        let response = get(&app(), "/healthz").await;
        assert_eq!(response.status, StatusCode::OK, "status");
        assert_eq!(response.body, json!({ "status": "ok" }), "body");
    }

    #[tokio::test]
    async fn create_returns_201_with_location_and_canonical_fields() {
        let app = app();
        let response = create(
            &app,
            json!({ "mac": "52-54-00-AB-CD-EF", "hostname": "Node-01.Rack-A" }),
        )
        .await;
        assert_eq!(response.status, StatusCode::CREATED, "status");
        assert_eq!(
            response.location.as_deref(),
            Some("/api/v1/machines/m-0000000000000001"),
            "location header"
        );
        assert_eq!(
            response.body,
            json!({
                "id": FIRST_ID,
                "mac": "52:54:00:ab:cd:ef",
                "hostname": "node-01.rack-a",
                "state": "new",
            }),
            "body"
        );
    }

    #[tokio::test]
    async fn hostname_is_optional() {
        let response = create(&app(), json!({ "mac": MAC })).await;
        assert_eq!(response.status, StatusCode::CREATED, "status");
        assert_eq!(response.body["hostname"], Value::Null, "hostname");
    }

    #[tokio::test]
    async fn list_returns_machines_in_id_order() {
        let app = app();
        assert_eq!(get(&app, "/api/v1/machines").await.body, json!([]), "empty");

        create(&app, json!({ "mac": "52:54:00:00:00:02" })).await;
        create(&app, json!({ "mac": "52:54:00:00:00:01" })).await;
        let response = get(&app, "/api/v1/machines").await;
        assert_eq!(response.status, StatusCode::OK, "status");
        let ids: Vec<&str> = response
            .body
            .as_array()
            .unwrap()
            .iter()
            .map(|machine| machine["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["m-0000000000000001", "m-0000000000000002"], "order");
    }

    #[tokio::test]
    async fn get_returns_the_created_machine() {
        let app = app();
        let created = create(&app, json!({ "mac": MAC })).await;
        let fetched = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        assert_eq!(fetched.status, StatusCode::OK, "status");
        assert_eq!(fetched.body, created.body, "body");
    }

    #[tokio::test]
    async fn get_unknown_machine_is_404() {
        let response = get(&app(), &format!("/api/v1/machines/{UNKNOWN_ID}")).await;
        assert_error(&response, StatusCode::NOT_FOUND, "machine_not_found");
    }

    #[tokio::test]
    async fn malformed_machine_id_is_404() {
        let response = get(&app(), "/api/v1/machines/42").await;
        assert_error(&response, StatusCode::NOT_FOUND, "invalid_machine_id");
    }

    #[tokio::test]
    async fn duplicate_mac_is_409_in_any_spelling() {
        let app = app();
        create(&app, json!({ "mac": MAC })).await;
        let response = create(&app, json!({ "mac": "52-54-00-12-34-56" })).await;
        assert_error(&response, StatusCode::CONFLICT, "duplicate_mac_address");
    }

    #[tokio::test]
    async fn malformed_mac_is_422() {
        let response = create(&app(), json!({ "mac": "52:54:00:12:34" })).await;
        assert_error(
            &response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_mac_address",
        );
    }

    #[tokio::test]
    async fn malformed_hostname_is_422() {
        let response = create(&app(), json!({ "mac": MAC, "hostname": "-node" })).await;
        assert_error(
            &response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_hostname",
        );
    }

    #[tokio::test]
    async fn missing_and_unknown_fields_are_422() {
        let app = app();
        for body in [json!({}), json!({ "mac": MAC, "rack": "a" })] {
            let response = create(&app, body).await;
            assert_error(
                &response,
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_request_body",
            );
        }
    }

    #[tokio::test]
    async fn invalid_json_is_400() {
        let body = Some("{not json".to_owned());
        let response = send(&app(), Method::POST, "/api/v1/machines", body).await;
        assert_error(&response, StatusCode::BAD_REQUEST, "invalid_request_body");
    }

    #[tokio::test]
    async fn rejected_create_allocates_no_machine() {
        let app = app();
        create(&app, json!({ "mac": "not-a-mac" })).await;
        let response = create(&app, json!({ "mac": MAC })).await;
        assert_eq!(
            response.body["id"], FIRST_ID,
            "first successful create gets id 1"
        );
    }

    #[tokio::test]
    async fn full_lifecycle_walk_via_events() {
        let app = app();
        create(&app, json!({ "mac": MAC })).await;

        let steps = [
            ("start_commissioning", "commissioning"),
            ("commissioning_succeeded", "ready"),
            ("allocate", "allocated"),
            ("deploy", "deploying"),
            ("deployment_succeeded", "deployed"),
            ("release", "releasing"),
            ("release_completed", "ready"),
        ];
        for (event, expected) in steps {
            let response = apply(&app, FIRST_ID, event).await;
            assert_eq!(response.status, StatusCode::OK, "status after `{event}`");
            assert_eq!(response.body["state"], expected, "state after `{event}`");
        }

        let fetched = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        assert_eq!(fetched.body["state"], "ready", "the final state is stored");
    }

    #[tokio::test]
    async fn invalid_transition_is_409_and_keeps_state() {
        let app = app();
        create(&app, json!({ "mac": MAC })).await;
        let response = apply(&app, FIRST_ID, "deploy").await;
        assert_error(&response, StatusCode::CONFLICT, "invalid_transition");

        let fetched = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        assert_eq!(fetched.body["state"], "new", "state is unchanged");
    }

    #[tokio::test]
    async fn event_for_unknown_machine_is_404() {
        let response = apply(&app(), UNKNOWN_ID, "allocate").await;
        assert_error(&response, StatusCode::NOT_FOUND, "machine_not_found");
    }

    #[tokio::test]
    async fn unknown_event_is_422() {
        let app = app();
        create(&app, json!({ "mac": MAC })).await;
        let response = apply(&app, FIRST_ID, "explode").await;
        assert_error(
            &response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_body",
        );
    }

    #[tokio::test]
    async fn unknown_route_is_404() {
        let response = get(&app(), "/api/v2/machines").await;
        assert_error(&response, StatusCode::NOT_FOUND, "route_not_found");
    }

    #[tokio::test]
    async fn unsupported_method_is_405() {
        let response = send(&app(), Method::DELETE, "/api/v1/machines", None).await;
        assert_error(
            &response,
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        );
    }
}
