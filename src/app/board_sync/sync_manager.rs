//! 画板同步管理器模块
//!
//! 该模块负责管理本地画板与服务器之间的同步，包括全量同步、增量同步
//! 和事件监听等功能。

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info};
use winter_paintboard_sdk::Rgb;

use super::local_board::LocalBoard;

/// 画板同步管理器
///
/// 负责协调本地画板与服务器之间的数据同步，包括全量同步、增量同步
/// 和实时事件处理等功能
#[derive(Clone)]
pub struct BoardSyncManager {
    /// 本地画板数据的Arc引用
    local_board: Arc<LocalBoard>,
    /// 停止标志，用于控制同步循环
    should_stop: Arc<RwLock<bool>>,
    /// 同步进行中标志，用于在同步期间暂停事件处理
    sync_in_progress: Arc<RwLock<bool>>,
}

impl BoardSyncManager {
    /// 创建新的同步管理器
    ///
    /// # 返回值
    ///
    /// 返回初始化的同步管理器实例
    pub fn new() -> Self {
        Self {
            local_board: Arc::new(LocalBoard::new(1000, 600)),
            should_stop: Arc::new(RwLock::new(false)),
            sync_in_progress: Arc::new(RwLock::new(false)),
        }
    }

    /// 使用已有的 LocalBoard 创建同步管理器
    ///
    /// # 参数
    ///
    /// * `local_board` - 已有的 LocalBoard 实例
    ///
    /// # 返回值
    ///
    /// 返回使用指定 LocalBoard 的同步管理器实例
    pub fn with_board(local_board: Arc<LocalBoard>) -> Self {
        Self {
            local_board,
            should_stop: Arc::new(RwLock::new(false)),
            sync_in_progress: Arc::new(RwLock::new(false)),
        }
    }

    /// 获取本地画板数据的Arc引用
    ///
    /// # 返回值
    ///
    /// 返回指向本地画板数据的Arc引用
    pub fn local_board(&self) -> Arc<LocalBoard> {
        self.local_board.clone()
    }

    /// 开始同步循环
    ///
    /// 启动一个后台任务，定期从服务器获取画板数据并与本地数据比较，
    /// 只更新发生变化的部分
    ///
    /// # 参数
    ///
    /// * `client` - 画板客户端
    /// * `sync_interval` - 同步间隔时间
    /// * `on_diff` - 当发现差异时的回调函数
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动增量同步循环
    /// * `Err` - 启动过程中发生错误
    pub async fn start_sync_loop<F>(
        &self,
        client: Arc<dyn winter_paintboard_sdk::PaintboardClientTrait + Send + Sync>,
        sync_interval: Duration,
        on_diff: Option<Arc<F>>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        F: Fn(Vec<(u32, u32, Rgb)>) + Send + Sync + 'static,
    {
        // 1. 立即执行一次初始同步,确保画板在使用前已初始化
        info!("执行初始全量同步...");
        match tokio::time::timeout(Duration::from_secs(30), client.get_board()).await {
            Ok(Ok(board_data)) => {
                // 使用 catch_unwind 保护同步代码,防止 panic
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.local_board.update_from_board(&board_data)
                })) {
                    Ok(changes) => {
                        info!("✅ 初始全量同步完成,画板已初始化");
                        // 初始同步通常量很大，不一定广播，但如果需要也可以广播
                        // 这里我们选择不广播初始同步（因为 Worker 刚连上会自己拿 FullBoard）
                        // 或者如果 Worker 已经在线，广播初始同步的变更也是合理的
                        if let Some(cb) = &on_diff {
                            cb(changes);
                        }
                    }
                    Err(e) => {
                        error!("❌ update_from_board panic: {:?}", e);
                        error!("初始同步失败,但将继续启动后台同步循环");
                    }
                }
            }
            Ok(Err(e)) => {
                error!("❌ 初始全量同步失败: {:?}", e);
                error!("将继续启动后台同步循环,稍后重试");
            }
            Err(_) => {
                error!("❌ 初始全量同步超时 (30秒)");
                error!("将继续启动后台同步循环,稍后重试");
            }
        }

        // 2. 启动后台同步循环
        let sync_manager = self.clone();
        let on_diff = on_diff.clone(); // Clone Arc

        // 移除 tokio::spawn，直接在当前任务中运行
        let mut interval_timer = interval(sync_interval);

        // 设置错过的 tick 策略：跳过错过的 tick，避免雪崩效应
        interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!("全量同步后台任务已启动，间隔: {:?}", sync_interval);

        loop {
            // 检查是否需要停止
            {
                let stop = sync_manager.should_stop.read().await;
                if *stop {
                    info!("检测到停止信号，同步任务退出");
                    break;
                }
                drop(stop); // 释放锁
            }

            // 等待下一个同步时间点
            interval_timer.tick().await;

            // 开始同步前，标记同步进行中
            {
                let mut sync_flag = sync_manager.sync_in_progress.write().await;
                *sync_flag = true;
            }

            info!("开始执行 HTTP 全量同步...");

            // 执行同步,添加超时保护
            match tokio::time::timeout(Duration::from_secs(30), client.get_board()).await {
                Ok(Ok(board_data)) => {
                    // 使用 catch_unwind 保护同步代码,防止 panic
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        sync_manager.local_board.update_from_board(&board_data)
                    })) {
                        Ok(changes) => {
                            info!("HTTP 全量同步完成");
                            if let Some(cb) = &on_diff {
                                cb(changes);
                            }
                        }
                        Err(e) => {
                            error!("❌ update_from_board panic: {:?}", e);
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("HTTP 全量同步失败: {:?}", e);
                }
                Err(_) => {
                    error!("HTTP 全量同步超时 (30秒)");
                }
            }

            // 同步完成后，标记同步结束
            {
                let mut sync_flag = sync_manager.sync_in_progress.write().await;
                *sync_flag = false;
            }
        }

        Ok(())
    }

    /// 停止同步管理器
    ///
    /// 停止所有同步任务并清理资源
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功停止同步管理器
    /// * `Err` - 停止过程中发生错误
    #[allow(dead_code)]
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut should_stop = self.should_stop.write().await;
            *should_stop = true;
        }
        info!("同步管理器已停止");
        Ok(())
    }
}
