//! 画板同步管理器模块
//!
//! 该模块负责管理本地画板与服务器之间的同步，包括全量同步、增量同步
//! 和事件监听等功能。

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info};

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
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动增量同步循环
    /// * `Err` - 启动过程中发生错误
    pub async fn start_sync_loop(
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

                        info!("增量同步完成，处理了服务器数据更新");
                    }
                    Err(e) => {
                        error!("增量同步失败: {:?}", e);
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
