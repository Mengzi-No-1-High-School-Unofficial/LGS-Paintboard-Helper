use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use super::pool::ConnectionPool;
use crate::config::Config;
use crate::error::PaintboardError;

/// 连接池管理器配置
#[derive(Debug, Clone)]
pub struct PoolManagerConfig {
    /// 监控间隔时间
    pub monitor_interval: Duration,
    /// 扩容阈值 (活跃连接率高于此值时扩容)
    pub scale_up_threshold: f64,
    /// 缩容阈值 (活跃连接率低于此值时缩容)
    pub scale_down_threshold: f64,
    /// 闲置连接超时时间
    pub idle_timeout: Duration,
    /// 预热连接比例 (基于历史使用模式)
    pub warmup_ratio: f64,
}

impl Default for PoolManagerConfig {
    fn default() -> Self {
        Self {
            monitor_interval: Duration::from_secs(5), // 每5秒检查一次
            scale_up_threshold: 0.8,                  // 活跃率超过80%扩容
            scale_down_threshold: 0.3,                // 活跃率低于30%缩容
            idle_timeout: Duration::from_secs(300),   // 5分钟无使用则回收
            warmup_ratio: 0.2,                        // 预热20%额外连接
        }
    }
}

/// 连接池管理器
pub struct PoolManager {
    pool: ConnectionPool,
    config: PoolManagerConfig,
    cancellation_token: CancellationToken,
}

impl PoolManager {
    /// 创建新的连接池管理器
    pub fn new(pool: ConnectionPool, config: PoolManagerConfig) -> Self {
        Self {
            pool,
            config,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// 启动连接池管理任务
    pub async fn start(&self) {
        let pool = self.pool.clone();
        let config = self.config.clone();
        let cancellation_token = self.cancellation_token.clone();

        tokio::spawn(async move {
            let mut interval = interval(config.monitor_interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = Self::manage_connections(&pool, &config).await {
                            warn!("连接池管理任务出错: {:?}", e); // 现在 e 是 PaintboardError
                        }
                    }
                    _ = cancellation_token.cancelled() => {
                        info!("连接池管理任务已取消");
                        break;
                    }
                }
            }
        });
    }

    /// 管理连接池的主要逻辑
    async fn manage_connections(
        pool: &ConnectionPool,
        config: &PoolManagerConfig,
    ) -> Result<(), PaintboardError> {
        let (pool_size, active_count) = pool.get_pool_stats().await;
        let total_connections = pool_size + active_count;

        if total_connections == 0 && pool.max_connections > 0 {
            // 如果连接池为空，且允许创建连接，尝试创建最小连接数
            for _ in 0..pool.min_connections {
                let _ = pool.create_new_connection().await; // 尝试创建，但不阻止流程
            }
        }

        // 计算活跃率
        let utilization_rate = if total_connections > 0 {
            active_count as f64 / total_connections as f64
        } else {
            0.0
        };

        debug!(
            "连接池状态 - 池中: {}, 活跃: {}, 总计: {}, 使用率: {:.2}%",
            pool_size,
            active_count,
            total_connections,
            utilization_rate * 100.0
        );

        // 检查是否需要扩容
        if Self::should_scale_up(
            utilization_rate,
            pool_size,
            pool.min_connections,
            pool.max_connections,
        ) {
            Self::scale_up(pool, config).await;
        }
        // 检查是否需要缩容
        else if Self::should_scale_down(
            utilization_rate,
            pool_size,
            pool.min_connections,
            pool.max_connections,
        ) {
            Self::scale_down(pool, config).await;
        }

        // 清理闲置连接
        Self::cleanup_idle_connections(pool, config).await;

        Ok(())
    }

    /// 判断是否需要扩容
    fn should_scale_up(
        utilization_rate: f64,
        current_pool_size: usize,
        min_connections: usize,
        max_connections: usize,
    ) -> bool {
        // 如果池中连接数少于最小连接数，需要扩容
        if current_pool_size < min_connections {
            info!(
                "池中连接数({})少于最小连接数({})，需要扩容",
                current_pool_size, min_connections
            );
            return true;
        }

        // 如果活跃率高于扩容阈值且未达到最大连接数，需要扩容
        if utilization_rate > 0.8 && current_pool_size < max_connections {
            info!("连接使用率过高({:.2}%)，需要扩容", utilization_rate * 100.0);
            return true;
        }

        false
    }

    /// 判断是否需要缩容
    fn should_scale_down(
        utilization_rate: f64,
        current_pool_size: usize,
        min_connections: usize,
        max_connections: usize,
    ) -> bool {
        // 只有当前池中连接数大于最小连接数时才考虑缩容
        if current_pool_size <= min_connections {
            return false;
        }

        // 如果活跃率低于缩容阈值，考虑缩容
        if utilization_rate < 0.3 {
            info!("连接使用率较低({:.2}%)，考虑缩容", utilization_rate * 100.0);
            return true;
        }

        false
    }

    /// 扩容逻辑
    async fn scale_up(pool: &ConnectionPool, config: &PoolManagerConfig) {
        let (current_pool_size, active_count) = pool.get_pool_stats().await;
        let total_connections = current_pool_size + active_count;

        // 计算需要创建的连接数
        let needed_connections = if current_pool_size < pool.min_connections {
            // 如果池中连接数少于最小连接数，补充到最小连接数
            pool.min_connections - current_pool_size
        } else {
            // 基于活跃连接数和预热比例计算需要的额外连接
            let extra_needed = (active_count as f64 * config.warmup_ratio) as usize;
            let target_pool_size =
                std::cmp::min(pool.max_connections, pool.min_connections + extra_needed);
            target_pool_size.saturating_sub(current_pool_size)
        };

        if needed_connections > 0 {
            let actual_created =
                std::cmp::min(needed_connections, pool.max_connections - current_pool_size);
            if actual_created > 0 {
                info!(
                    "开始扩容: 池中当前{}个连接，计划创建{}个新连接",
                    current_pool_size, actual_created
                );

                // 创建新的连接并放入池中
                for i in 0..actual_created {
                    match pool.create_new_connection().await {
                        Ok(mut client) => {
                            // 设置认证信息
                            if let (Some(uid), Some(token)) = (pool.uid, pool.token.as_ref()) {
                                use crate::PaintboardClientTrait; // 确保有 set_auth 方法
                                client.set_auth(uid, token.clone());
                            }
                            pool.add_connection_to_pool(client).await;
                            info!("扩容成功 - 已创建并添加第{}个新连接", i + 1);
                        }
                        Err(e) => {
                            warn!("创建新连接失败: {:?}", e);
                            // 即使创建失败也继续尝试其他连接
                        }
                    }
                }
            }
        }
    }

    /// 缩容逻辑
    async fn scale_down(pool: &ConnectionPool, config: &PoolManagerConfig) {
        let current_pool_size = pool.pool_size().await;
        let min_connections = pool.min_connections;

        if current_pool_size <= min_connections {
            return; // 不能缩容到最小连接数以下
        }

        // 计算可以安全回收的连接数
        let connections_to_keep =
            std::cmp::max(min_connections, (current_pool_size as f64 * 0.7) as usize);
        let connections_to_remove = current_pool_size.saturating_sub(connections_to_keep);

        if connections_to_remove > 0 {
            info!(
                "开始缩容: 当前池中{}个连接，计划回收{}个连接",
                current_pool_size, connections_to_remove
            );

            // 实际回收连接（通过清理闲置连接实现）
            pool.cleanup_inactive_connections(config.idle_timeout).await;
        }
    }

    /// 清理闲置连接
    async fn cleanup_idle_connections(pool: &ConnectionPool, config: &PoolManagerConfig) {
        pool.cleanup_inactive_connections(config.idle_timeout).await;
    }

    /// 停止管理器
    pub fn stop(&self) {
        self.cancellation_token.cancel();
    }
}

/// 便捷函数：启动连接池管理任务
/// 如果传入的 config 为 None，则视为显式禁用自动管理（保持固定连接数），将不会启动后台管理任务。
pub async fn start_connection_manager_task(
    pool: ConnectionPool,
    config: Option<PoolManagerConfig>,
) {
    if config.is_none() {
        info!("连接池管理器已被禁用（config 为 None），保持固定连接数，跳过启动");
        return;
    }
    let config = config.unwrap();
    let manager = PoolManager::new(pool, config);
    manager.start().await;
}
