//! 画板同步管理器模块
//!
//! 该模块负责管理本地画板与服务器之间的同步，包括全量同步、增量同步
//! 和事件监听等功能。

use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use winter_paintboard_sdk::PaintboardClientTrait;

use super::local_board::{LocalBoard, SyncStatus};

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

    /// 获取本地画板数据的Arc引用
    ///
    /// # 返回值
    ///
    /// 返回指向本地画板数据的Arc引用
    pub fn local_board(&self) -> Arc<LocalBoard> {
        self.local_board.clone()
    }

    /// 开始全量同步循环
    ///
    /// 启动一个后台任务，定期从服务器获取完整的画板数据并更新本地数据
    ///
    /// # 参数
    ///
    /// * `client` - 画板客户端
    /// * `sync_interval` - 同步间隔时间
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动同步循环
    /// * `Err` - 启动过程中发生错误
    pub async fn start_sync_loop(
        &self,
        client: Box<dyn winter_paintboard_sdk::PaintboardClientTrait + Send>,
        sync_interval: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sync_manager = self.clone(); // 将self包装为Arc以在异步任务中使用
        let client = client; // 确保client是可变的

        tokio::spawn(async move {
            let mut interval_timer = interval(sync_interval);

            loop {
                // 检查是否需要停止
                {
                    let stop = sync_manager.should_stop.read().await;
                    if *stop {
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
                    info!("开始全量同步绘版数据...");
                }

                // 尝试同步，带重试机制
                let mut attempt = 0;
                let max_retries = 3;
                let mut success = false;

                while attempt < max_retries && !success {
                    match client.get_board().await {
                        Ok(board_data) => {
                            {
                                let board = sync_manager.local_board.clone();
                                board.update_from_board(&board_data);
                                board.set_sync_status(SyncStatus::Idle);
                                info!(
                                    "全量同步完成，获取到 {} 个像素数据",
                                    board_data.width as u32 * board_data.height as u32
                                );
                            }
                            success = true;
                        }
                        Err(e) => {
                            attempt += 1;
                            if attempt >= max_retries {
                                error!(
                                    "全量同步失败，已达到最大重试次数({}): {:?}",
                                    max_retries, e
                                );
                                {
                                    let board = sync_manager.local_board.clone();
                                    board.set_sync_status(SyncStatus::Error(e.to_string()));
                                }
                            } else {
                                warn!("全量同步失败，第 {} 次重试，错误: {:?}", attempt, e);
                                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
                                // 指数退避
                            }
                        }
                    }
                }

                // 同步完成后，标记同步结束
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = false;
                }

                // 同步期间不再缓存事件
            }
        });

        Ok(())
    }

    /// 开始增量同步循环 - 只同步发生变化的区域
    ///
    /// 启动一个后台任务，定期从服务器获取画板数据并与本地数据比较，
    /// 只更新发生变化的部分
    ///
    /// # 参数
    ///
    /// * `client` - 画板客户端
    /// * `sync_interval` - 同步间隔时间
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动增量同步循环
    /// * `Err` - 启动过程中发生错误
    pub async fn start_incremental_sync_loop(
        &self,
        client: Arc<dyn winter_paintboard_sdk::PaintboardClientTrait + Send + Sync>,
        sync_interval: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sync_manager = self.clone();
        let client = client; // 确保client是可变的

        // 不依赖本地版本号进行判断，而是总是获取服务器数据并进行比较
        // 或者可以记录服务器数据的某种标识（如校验和）来判断是否有变化

        tokio::spawn(async move {
            let mut interval_timer = interval(sync_interval);

            loop {
                // 检查是否需要停止
                {
                    let stop = sync_manager.should_stop.read().await;
                    if *stop {
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
                    info!("开始增量同步绘版数据...");
                }

                // 尝试获取服务器数据
                match client.get_board().await {
                    Ok(board_data) => {
                        let board = sync_manager.local_board.clone();

                        // 执行差异同步（update_from_board 方法会进行实际的差异比较和更新）
                        board.update_from_board(&board_data);

                        // 验证同步后的数据一致性
                        if !board.verify_integrity() {
                            warn!("数据完整性验证失败，可能需要全量同步");
                        }

                        board.set_sync_status(SyncStatus::Idle);
                        info!("增量同步完成，处理了服务器数据更新");
                    }
                    Err(e) => {
                        error!("增量同步失败: {:?}", e);
                        {
                            let board = sync_manager.local_board.clone();
                            board.set_sync_status(SyncStatus::Error(e.to_string()));
                        }
                    }
                }

                // 同步完成后，标记同步结束
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = false;
                }

                // 同步期间不再缓存事件
            }
        });

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
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut should_stop = self.should_stop.write().await;
            *should_stop = true;
        }
        info!("同步管理器已停止");
        Ok(())
    }
}
