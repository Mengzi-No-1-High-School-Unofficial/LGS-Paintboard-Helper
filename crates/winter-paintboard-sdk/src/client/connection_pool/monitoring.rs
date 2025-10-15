use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time;

use super::{PoolMetrics, ConnectionPool};

// 业务层监控任务
pub async fn start_monitoring_task(
    metrics: Arc<Mutex<PoolMetrics>>, 
    interval: std::time::Duration
) {
    tokio::spawn(async move {
        let mut interval = time::interval(interval);
        
        loop {
            interval.tick().await;
            
            // 从共享的Arc<Mutex<PoolMetrics>>中获取最新数据
            let metrics_snapshot = {
                let m = metrics.lock().await;
                m.clone()  // 这里只需要克隆快照，而不是频繁操作原数据
            };
            
            print_metrics(&metrics_snapshot);
        }
    });
}

// 打印监控数据的函数
fn print_metrics(metrics: &PoolMetrics) {
    println!("=== Connection Pool 监控数据 ===");
    println!("池中连接数: {}/{}", metrics.pool_size, metrics.max_connections);
    println!("活跃连接数: {}", metrics.active_count);
    println!("总请求数: {}", metrics.total_requests);
    println!("总发包数: {}", metrics.total_packets);
    println!("批量请求次数: {}", metrics.batch_requests);
    println!("总错误数: {}", metrics.total_errors);
    println!("错误率: {:.2}%", metrics.error_rate());
    println!("总重试次数: {}", metrics.retry_count);
    println!("重试成功率: {:.2}%", metrics.retry_success_rate());
    println!("损坏连接数: {}", metrics.broken_count);
    println!("创建连接数: {}", metrics.created_count);
    println!("释放连接数: {}", metrics.released_count);
    println!("=========================");
}

impl ConnectionPool {
    // 获取监控数据的Arc引用
    pub fn metrics(&self) -> Arc<Mutex<PoolMetrics>> {
        Arc::clone(&self.metrics)
    }
    
    // 获取实时监控数据
    pub async fn get_metrics(&self) -> PoolMetrics {
        let mut metrics = self.metrics().lock().await.clone();
        metrics.pool_size = self.pool_size().await;
        metrics.active_count = self.active_count().await;
        metrics.min_connections = self.min_connections;
        metrics.max_connections = self.max_connections;
        metrics.timestamp = std::time::SystemTime::now();
        metrics
    }
}

// 提供对内部连接池的只读访问
impl super::PoolClient {
    pub fn pool(&self) -> &ConnectionPool {
        &self.pool
    }
}