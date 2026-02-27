use crate::middleware::auth::TeamContext;
use crate::server::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

pub async fn team_policy(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let team_ctx = req.extensions().get::<TeamContext>().cloned();

    if let Some(ctx) = team_ctx {
        let (rpm_limit, tpm_limit, team_id) = {
            let config = state.config.read().unwrap();
            if let Some(team) = config.teams.iter().find(|t| t.id == ctx.team_id) {
                let policy = &team.policy;
                let rpm = policy
                    .rate_limit
                    .as_ref()
                    .and_then(|l| l.rpm)
                    .filter(|&v| v > 0)
                    .map(|v| v as u32);
                let tpm = policy
                    .rate_limit
                    .as_ref()
                    .and_then(|l| l.tpm)
                    .filter(|&v| v > 0)
                    .map(|v| v as u32);
                (rpm, tpm, Some(team.id.clone()))
            } else {
                (None, None, None)
            }
        };

        let limit_exceeded = if let Some(id) = &team_id {
            (rpm_limit.is_some() || tpm_limit.is_some())
                && !state.team_rate_limiter.check(id, rpm_limit, tpm_limit, 100)
        } else {
            false
        };

        if limit_exceeded {
            let id = team_id.as_ref().unwrap();
            tracing::warn!("Rate Limit Exceeded: Team '{}'", id);
            return Err(Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"error": "Rate limit exceeded"}"#))
                .unwrap());
        }
    }

    Ok(next.run(req).await)
}
