//! The legacy `/api/*` read endpoints: usage, metrics and the two ranking /
//! trend aggregates the control plane still reads.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};

use super::AppState;

pub(super) async fn usage_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let team_id = params.get("team_id").map(|s| s.as_str());
    let router = params.get("router").map(|s| s.as_str());
    let channel = params.get("channel").map(|s| s.as_str());
    let model = params.get("model").map(|s| s.as_str());
    let status = params.get("status").map(|s| s.as_str());
    let start_date = params.get("start_date").map(|s| s.as_str());
    let end_date = params.get("end_date").map(|s| s.as_str());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .min(100);
    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    match state.database.get_usage_records(
        team_id, router, channel, model, status, start_date, end_date, limit, offset,
    ) {
        Ok((records, total)) => {
            let json = serde_json::json!({
                "data": records,
                "total": total,
                "limit": limit,
                "offset": offset
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

pub(super) async fn metrics_api_handler(state: State<Arc<AppState>>) -> Response<Body> {
    match state.database.get_metrics_summary() {
        Ok(summary) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&summary).unwrap_or_default(),
            ))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

pub(super) async fn trends_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let period = params.get("period").map(|s| s.as_str()).unwrap_or("daily");
    let start_date = params.get("start_date").map(|s| s.as_str());
    let end_date = params.get("end_date").map(|s| s.as_str());

    match state.database.get_trends(period, start_date, end_date) {
        Ok(trends) => {
            let json = serde_json::json!({
                "period": period,
                "data": trends
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

pub(super) async fn rankings_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let by = params.get("by").map(|s| s.as_str()).unwrap_or("team_id");
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(10);

    match state.database.get_rankings(by, limit) {
        Ok(rankings) => {
            let json = serde_json::json!({
                "by": by,
                "data": rankings
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}
