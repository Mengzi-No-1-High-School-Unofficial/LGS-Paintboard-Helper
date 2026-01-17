//! 监控 API 端点
//!
//! 提供监控数据查询的 HTTP API

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::app::ipc::{MetricsAggregator, MetricsSummary, WorkerMetrics};

/// API 共享状态
#[derive(Clone)]
pub struct ApiState {
    /// 监控聚合器
    pub aggregator: Arc<MetricsAggregator>,
}

/// 创建监控路由
pub fn create_metrics_router(aggregator: Arc<MetricsAggregator>) -> Router {
    let state = ApiState { aggregator };

    Router::new()
        .route("/api/metrics/summary", get(get_summary))
        .route("/api/metrics/workers", get(get_all_workers))
        .route("/api/metrics/worker/:worker_id", get(get_worker))
        .route(
            "/api/metrics/worker/:worker_id/history",
            get(get_worker_history),
        )
        .route("/api/metrics/health", get(get_health))
        .with_state(state)
}

/// GET /api/metrics/summary - 获取全局监控摘要
async fn get_summary(State(state): State<ApiState>) -> Json<MetricsSummary> {
    Json(state.aggregator.get_summary())
}

/// GET /api/metrics/workers - 获取所有 Worker 的最新指标
async fn get_all_workers(State(state): State<ApiState>) -> Json<WorkersResponse> {
    let workers = state.aggregator.get_all_latest_metrics();
    Json(WorkersResponse { workers })
}

#[derive(Serialize)]
struct WorkersResponse {
    workers: Vec<WorkerMetrics>,
}

/// GET /api/metrics/worker/{worker_id} - 获取指定 Worker 的最新指标
async fn get_worker(
    State(state): State<ApiState>,
    Path(worker_id): Path<String>,
) -> Result<Json<WorkerMetrics>, StatusCode> {
    state
        .aggregator
        .get_worker_metrics(&worker_id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// GET /api/metrics/worker/{worker_id}/history?duration=1h - 获取 Worker 历史数据
async fn get_worker_history(
    State(state): State<ApiState>,
    Path(worker_id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let duration_str = params.duration.clone().unwrap_or_else(|| "1h".to_string());
    let duration_secs = parse_duration(&duration_str).map_err(|_| StatusCode::BAD_REQUEST)?;

    let metrics = state
        .aggregator
        .get_worker_history(&worker_id, duration_secs)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(HistoryResponse {
        worker_id,
        duration: duration_str,
        data_points: metrics,
    }))
}

#[derive(Deserialize)]
struct HistoryParams {
    duration: Option<String>,
}

#[derive(Serialize)]
struct HistoryResponse {
    worker_id: String,
    duration: String,
    data_points: Vec<WorkerMetrics>,
}

/// 解析时长字符串（1h, 6h, 24h, 7d 等）
fn parse_duration(duration_str: &str) -> Result<u64, String> {
    let duration_str = duration_str.trim().to_lowercase();

    if let Some(value) = duration_str.strip_suffix('h') {
        let hours: u64 = value
            .parse()
            .map_err(|_| "Invalid hour value".to_string())?;
        Ok(hours * 3600)
    } else if let Some(value) = duration_str.strip_suffix('d') {
        let days: u64 = value.parse().map_err(|_| "Invalid day value".to_string())?;
        Ok(days * 86400)
    } else {
        Err("Invalid duration format. Use '1h', '24h', '7d', etc.".to_string())
    }
}

/// GET /api/metrics/health - 健康检查
async fn get_health(State(state): State<ApiState>) -> Json<HealthResponse> {
    let uptime_secs = state.aggregator.get_master_uptime();
    let workers = state.aggregator.get_all_latest_metrics();

    Json(HealthResponse {
        status: "healthy".to_string(),
        master_uptime_secs: uptime_secs,
        database_status: "connected".to_string(),
        total_workers: workers.len(),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    master_uptime_secs: u64,
    database_status: String,
    total_workers: usize,
}
