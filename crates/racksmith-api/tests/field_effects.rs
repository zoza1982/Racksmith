//! Field-effect tests: every request input changes an observable outcome.
//!
//! Each test runs the same workload twice against a fresh in-memory router, once with one input
//! changed, and asserts that the responses differ. No sockets, clock or randomness are used, so
//! a test that passes once passes every time. An input whose change no test can observe is
//! accepted and ignored, which the completeness layer treats as a defect.

// Dependencies of the library that these tests do not use directly.
use serde as _;
use thiserror as _;
use tracing as _;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use http_body_util::BodyExt as _;
    use racksmith_api::{AppState, router};
    use racksmith_domain::{MachineEvent, MachineState};
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    const MAC: &str = "52:54:00:12:34:56";
    const OTHER_MAC: &str = "52:54:00:12:34:57";
    const FIRST_ID: &str = "m-0000000000000001";
    const SECOND_ID: &str = "m-0000000000000002";

    /// The observable outcome of one request: its status and JSON body.
    type Outcome = (StatusCode, Value);

    fn app() -> Router {
        router(AppState::default())
    }

    /// Sends `body` as JSON to `uri` and returns the status and decoded JSON response.
    async fn post(app: &Router, uri: &str, body: &Value) -> Outcome {
        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn get(app: &Router, uri: &str) -> Outcome {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn create(app: &Router, body: &Value) -> Outcome {
        post(app, "/api/v1/machines", body).await
    }

    async fn apply(app: &Router, event: MachineEvent) -> Outcome {
        let uri = format!("/api/v1/machines/{FIRST_ID}/events");
        post(app, &uri, &json!({ "event": event.as_str() })).await
    }

    /// Registers one machine on a fresh router and returns the router with the stored machine.
    async fn create_and_fetch(body: &Value) -> (Outcome, Outcome) {
        let app = app();
        let created = create(&app, body).await;
        let fetched = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        (created, fetched)
    }

    /// The shortest event sequence that takes a new machine to each state.
    fn paths_to_every_state() -> BTreeMap<MachineState, Vec<MachineEvent>> {
        let mut paths = BTreeMap::from([(MachineState::New, Vec::new())]);
        let mut queue = VecDeque::from([MachineState::New]);
        while let Some(state) = queue.pop_front() {
            let path = paths[&state].clone();
            for event in MachineEvent::ALL {
                if let Ok(next) = state.apply(event) {
                    paths.entry(next).or_insert_with(|| {
                        queue.push_back(next);
                        let mut extended = path.clone();
                        extended.push(event);
                        extended
                    });
                }
            }
        }
        assert_eq!(
            paths.len(),
            MachineState::ALL.len(),
            "every state is reachable"
        );
        paths
    }

    /// Registers a machine, drives it to `state` over HTTP, applies `event` and returns the
    /// outcome of that last request.
    async fn outcome_of(path: &[MachineEvent], event: MachineEvent) -> Outcome {
        let app = app();
        let (created, _) = create(&app, &json!({ "mac": MAC })).await;
        assert_eq!(created, StatusCode::CREATED, "setup create");
        for &step in path {
            let (status, body) = apply(&app, step).await;
            assert_eq!(status, StatusCode::OK, "setup step `{step}`: {body}");
        }
        apply(&app, event).await
    }

    #[tokio::test]
    async fn mac_changes_the_created_machine() {
        let (created, fetched) = create_and_fetch(&json!({ "mac": MAC })).await;
        let (other_created, other_fetched) = create_and_fetch(&json!({ "mac": OTHER_MAC })).await;

        assert_eq!(created.0, StatusCode::CREATED, "status");
        assert_eq!(created.1["mac"], MAC, "response echoes the MAC");
        assert_eq!(other_created.1["mac"], OTHER_MAC, "response echoes the MAC");
        assert_ne!(
            created, other_created,
            "a different MAC changes the response"
        );
        assert_ne!(
            fetched, other_fetched,
            "a different MAC changes the stored machine"
        );
    }

    #[tokio::test]
    async fn mac_decides_whether_a_second_registration_conflicts() {
        let same = app();
        create(&same, &json!({ "mac": MAC })).await;
        let (repeated, _) = create(&same, &json!({ "mac": MAC })).await;

        let distinct = app();
        create(&distinct, &json!({ "mac": MAC })).await;
        let (other, _) = create(&distinct, &json!({ "mac": OTHER_MAC })).await;

        assert_eq!(
            repeated,
            StatusCode::CONFLICT,
            "the same MAC twice conflicts"
        );
        assert_eq!(other, StatusCode::CREATED, "a distinct MAC is registered");
    }

    #[tokio::test]
    async fn hostname_changes_the_created_machine() {
        let workloads = [
            json!({ "mac": MAC }),
            json!({ "mac": MAC, "hostname": "node-01" }),
            json!({ "mac": MAC, "hostname": "node-02.rack-a" }),
        ];
        let mut outcomes = Vec::new();
        for body in &workloads {
            let (created, fetched) = create_and_fetch(body).await;
            assert_eq!(created.0, StatusCode::CREATED, "status for {body}");
            assert_eq!(
                created.1["hostname"],
                body.get("hostname").cloned().unwrap_or(Value::Null),
                "response carries the hostname of {body}"
            );
            assert_eq!(created.1, fetched.1, "the hostname is stored for {body}");
            outcomes.push(created.1);
        }
        let distinct: BTreeSet<String> = outcomes.iter().map(Value::to_string).collect();
        assert_eq!(
            distinct.len(),
            workloads.len(),
            "each hostname, and its absence, gives a different machine: {outcomes:?}"
        );
    }

    /// Registers two machines with different MACs on a fresh router.
    async fn two_machines() -> Router {
        let app = app();
        for (mac, id) in [(MAC, FIRST_ID), (OTHER_MAC, SECOND_ID)] {
            let (status, body) = create(&app, &json!({ "mac": mac })).await;
            assert_eq!(status, StatusCode::CREATED, "setup create {mac}");
            assert_eq!(body["id"], id, "setup id of {mac}");
        }
        app
    }

    /// `{id}` of `GET /api/v1/machines/{id}` selects which machine is read.
    #[tokio::test]
    async fn path_id_selects_the_machine_that_is_read() {
        let app = two_machines().await;
        let first = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        let second = get(&app, &format!("/api/v1/machines/{SECOND_ID}")).await;
        let missing = get(&app, "/api/v1/machines/m-0000000000000003").await;
        assert_eq!(first.1["mac"], MAC, "first machine: {}", first.1);
        assert_eq!(second.1["mac"], OTHER_MAC, "second machine: {}", second.1);
        assert_eq!(missing.0, StatusCode::NOT_FOUND, "unknown id");
    }

    /// `{id}` of `POST /api/v1/machines/{id}/events` selects which machine the event changes:
    /// the addressed machine moves, the other one does not.
    #[tokio::test]
    async fn path_id_selects_the_machine_the_event_applies_to() {
        let app = two_machines().await;
        let event = json!({ "event": MachineEvent::StartCommissioning.as_str() });
        let (status, applied) = post(
            &app,
            &format!("/api/v1/machines/{SECOND_ID}/events"),
            &event,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{applied}");
        assert_eq!(
            applied["id"], SECOND_ID,
            "the response is the addressed machine"
        );

        let first = get(&app, &format!("/api/v1/machines/{FIRST_ID}")).await;
        let second = get(&app, &format!("/api/v1/machines/{SECOND_ID}")).await;
        assert_eq!(
            first.1["state"],
            MachineState::New.as_str(),
            "the other machine is unchanged"
        );
        assert_eq!(
            second.1["state"],
            MachineState::Commissioning.as_str(),
            "the addressed machine moved"
        );
    }

    /// For every pair of distinct events there is a state in which applying one instead of the
    /// other changes the response, so every `MachineEvent` variant sent as `event` has an
    /// effect. Each outcome must also match the domain's transition function.
    #[tokio::test]
    async fn event_changes_the_resulting_state() {
        let paths = paths_to_every_state();
        let mut outcomes = BTreeMap::new();
        for (&state, path) in &paths {
            for event in MachineEvent::ALL {
                let (status, body) = outcome_of(path, event).await;
                if let Ok(next) = state.apply(event) {
                    assert_eq!(status, StatusCode::OK, "`{event}` in `{state}`: {body}");
                    assert_eq!(body["state"], next.as_str(), "`{event}` in `{state}`");
                } else {
                    assert_eq!(status, StatusCode::CONFLICT, "`{event}` in `{state}`");
                    assert_eq!(body["error"]["code"], "invalid_transition", "{body}");
                }
                // Compare the resulting state or error code, not the message: an error message
                // names the event, which would tell any two rejected events apart.
                let result = body.get("state").unwrap_or(&body["error"]["code"]).clone();
                outcomes.insert((state, event), (status, result));
            }
        }

        for event in MachineEvent::ALL {
            for other in MachineEvent::ALL
                .into_iter()
                .filter(|&other| other != event)
            {
                let distinguishing = paths
                    .keys()
                    .find(|&&state| outcomes[&(state, event)] != outcomes[&(state, other)]);
                assert!(
                    distinguishing.is_some(),
                    "no state tells `{event}` apart from `{other}`"
                );
            }
        }
    }
}
