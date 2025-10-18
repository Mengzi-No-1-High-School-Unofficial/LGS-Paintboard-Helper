# Multi Token 代码重构实施计划

## 一、重构总览

### 目标
解决当前实现中的 4 个严重并发问题和 3 个关键逻辑问题，同时遵守服务端单连接限制。

### 核心策略
**单连接 + 请求队列 + 非阻塞 Token 管理**

### 工作量估算
- 总工时: 8-11 天
- 优先级: P0（关键缺陷修复）
- 影响范围: `src/app/multi_token/` 全部文件

---

## 二、详细实施步骤

### 📦 阶段 1: 准备和依赖更新（0.5 天）

#### 步骤 1.1: 更新 Cargo.toml
```toml
[dependencies]
# 添加同步原语
parking_lot = "0.12"  # 高性能 Mutex/RwLock

# 现有依赖保持
tokio = { version = "1", features = ["full"] }
log = "0.4"
```

#### 步骤 1.2: 创建新的模块结构
```bash
src/app/multi_token/
├── mod.rs                      # 模块导出
├── config.rs                   # 配置（保持不变）
├── cli.rs                      # CLI（保持不变）
├── token_manager.rs            # Token 管理器（重构）
├── token_lease.rs              # NEW: Token 租约 RAII 守卫
├── paint_request.rs            # NEW: 绘制请求结构
├── paint_executor.rs           # NEW: 单线程绘制执行器
├── pixel_queue.rs              # 像素队列（优化）
├── token_worker.rs             # Worker（简化）
└── multi_token_service.rs      # 服务入口（重构）
```

---

### 🔧 阶段 2: 重构 TokenManager（1.5 天）

#### 步骤 2.1: 创建 [`token_lease.rs`](token_lease.rs)

**目标**: 实现 RAII 守卫，自动管理 Token 生命周期

```rust
use std::time::{Duration, Instant};
use std::sync::Arc;
use parking_lot::Mutex;

/// Token 状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenState {
    Available,
    Acquired,
    InCooldown,
}

/// Token 租约（RAII 守卫）
pub struct TokenLease {
    index: usize,
    uid: u32,
    token: String,
    manager: Arc<Mutex<Vec<TokenData>>>,
    cd_duration: Duration,
    consumed: bool,  // 防止重复消费
}

impl TokenLease {
    pub fn uid(&self) -> u32 { self.uid }
    pub fn token(&self) -> &str { &self.token }
    
    /// 标记绘制成功，启动 CD
    pub fn mark_success(mut self) {
        self.consumed = true;
        let mut tokens = self.manager.lock();
        if let Some(token) = tokens.get_mut(self.index) {
            token.state = TokenState::InCooldown;
            token.cd_end_time = Some(Instant::now() + self.cd_duration);
        }
    }
    
    /// 标记绘制失败，释放 Token
    pub fn mark_failed(mut self) {
        self.consumed = true;
        let mut tokens = self.manager.lock();
        if let Some(token) = tokens.get_mut(self.index) {
            token.state = TokenState::Available;
        }
    }
}

impl Drop for TokenLease {
    fn drop(&mut self) {
        if !self.consumed {
            // 未消费，默认释放
            let mut tokens = self.manager.lock();
            if let Some(token) = tokens.get_mut(self.index) {
                if token.state == TokenState::Acquired {
                    token.state = TokenState::Available;
                }
            }
        }
    }
}
```

**测试用例**:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_token_lease_auto_release() {
        // 测试 Drop 自动释放
    }
    
    #[test]
    fn test_token_lease_mark_success() {
        // 测试成功标记启动 CD
    }
}
```

---

#### 步骤 2.2: 重构 [`token_manager.rs`](src/app/multi_token/token_manager.rs)

**变更点**:
1. 使用 `parking_lot::Mutex` 替代 `tokio::sync::Mutex`
2. 实现非阻塞 [`try_acquire()`](try_acquire())
3. 移除阻塞的 [`acquire_token()`](acquire_token())
4. 添加状态更新逻辑

```rust
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) struct TokenData {
    pub uid: u32,
    pub token: String,
    pub state: TokenState,
    pub cd_end_time: Option<Instant>,
}

pub struct TokenManager {
    tokens: Arc<Mutex<Vec<TokenData>>>,
    cd_duration: Duration,
}

impl TokenManager {
    pub fn new(tokens: Vec<TokenInfo>, cd_duration: Duration) -> Self {
        let token_data = tokens.into_iter()
            .map(|t| TokenData {
                uid: t.uid,
                token: t.token,
                state: TokenState::Available,
                cd_end_time: None,
            })
            .collect();
        
        Self {
            tokens: Arc::new(Mutex::new(token_data)),
            cd_duration,
        }
    }
    
    /// 非阻塞获取 Token
    pub fn try_acquire(&self) -> Option<TokenLease> {
        let mut tokens = self.tokens.lock();
        let now = Instant::now();
        
        // 1. 更新 CD 状态
        for token in tokens.iter_mut() {
            if token.state == TokenState::InCooldown {
                if let Some(end_time) = token.cd_end_time {
                    if now >= end_time {
                        token.state = TokenState::Available;
                        token.cd_end_time = None;
                    }
                }
            }
        }
        
        // 2. 查找可用 Token
        for (index, token) in tokens.iter_mut().enumerate() {
            if token.state == TokenState::Available {
                token.state = TokenState::Acquired;
                return Some(TokenLease::new(
                    index,
                    token.uid,
                    token.token.clone(),
                    Arc::clone(&self.tokens),
                    self.cd_duration,
                ));
            }
        }
        
        None
    }
    
    /// 获取下一个 Token 可用的最短等待时间
    pub fn next_available_in(&self) -> Option<Duration> {
        let tokens = self.tokens.lock();
        let now = Instant::now();
        
        tokens.iter()
            .filter_map(|token| {
                match token.state {
                    TokenState::Available => Some(Duration::ZERO),
                    TokenState::InCooldown => {
                        token.cd_end_time
                            .and_then(|end| end.checked_duration_since(now))
                    }
                    TokenState::Acquired => None,
                }
            })
            .min()
    }
}
```

**测试用例**:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_try_acquire_when_available() { }
    
    #[test]
    fn test_try_acquire_when_all_in_cd() { }
    
    #[test]
    fn test_cd_expiration() { }
    
    #[test]
    fn test_concurrent_acquire() {
        // 使用 loom 进行并发测试
    }
}
```

---

### 🚀 阶段 3: 实现 PaintExecutor（2 天）

#### 步骤 3.1: 创建 [`paint_request.rs`](paint_request.rs)

```rust
use crate::app::multi_token::config::PriorityPixel;
use crate::app::multi_token::token_lease::TokenLease;

/// 绘制请求（携带 Token 租约）
#[derive(Debug)]
pub struct PaintRequest {
    pub pixel: PriorityPixel,
    pub token_lease: TokenLease,
    pub retry_count: u32,
    pub max_retries: u32,
}

impl PaintRequest {
    pub fn new(pixel: PriorityPixel, token_lease: TokenLease) -> Self {
        Self {
            pixel,
            token_lease,
            retry_count: 0,
            max_retries: 3,
        }
    }
    
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }
    
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}
```

---

#### 步骤 3.2: 创建 [`paint_executor.rs`](paint_executor.rs)

**目标**: 串行化所有绘制请求，避免状态竞争

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use log::{debug, error, info, warn};

use winter_paintboard_sdk::{PaintboardClientTrait, models::PaintStatus};
use crate::app::board_sync::local_board::{LocalBoard, PixelSource};
use super::paint_request::PaintRequest;

/// 绘制请求队列
pub struct PaintRequestQueue {
    sender: mpsc::UnboundedSender<PaintRequest>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<PaintRequest>>>,
}

impl PaintRequestQueue {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
    
    pub fn send(&self, request: PaintRequest) -> Result<(), String> {
        self.sender.send(request)
            .map_err(|e| format!("发送绘制请求失败: {}", e))
    }
    
    pub async fn recv(&self) -> Option<PaintRequest> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }
}

/// 单线程绘制执行器
pub struct PaintExecutor {
    client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
    request_queue: Arc<PaintRequestQueue>,
    local_board: Arc<Mutex<LocalBoard>>,
}

impl PaintExecutor {
    pub fn new(
        client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
        request_queue: Arc<PaintRequestQueue>,
        local_board: Arc<Mutex<LocalBoard>>,
    ) -> Self {
        Self {
            client,
            request_queue,
            local_board,
        }
    }
    
    /// 启动执行循环
    pub async fn run(&self, stop_signal: Arc<AtomicBool>) {
        info!("PaintExecutor 启动");
        
        while !stop_signal.load(Ordering::Relaxed) {
            let request = match self.request_queue.recv().await {
                Some(req) => req,
                None => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
            };
            
            self.process_request(request).await;
        }
        
        info!("PaintExecutor 停止");
    }
    
    /// 处理单个绘制请求
    async fn process_request(&self, mut request: PaintRequest) {
        let result = {
            let mut client = self.client.lock().await;
            client.paint_with_token(
                request.pixel.pos,
                request.pixel.color,
                request.token_lease.uid(),
                request.token_lease.token().to_string(),
            ).await
        };
        
        match result {
            Ok(paint_result) => {
                match paint_result.status {
                    PaintStatus::Success => {
                        // 更新本地绘版
                        {
                            let mut board = self.local_board.lock().await;
                            board.update_pixel(
                                request.pixel.pos.x,
                                request.pixel.pos.y,
                                request.pixel.color,
                                PixelSource::Own,
                            );
                        }
                        
                        // 标记成功，启动 CD
                        request.token_lease.mark_success();
                        
                        debug!("绘制成功: ({}, {})", 
                            request.pixel.pos.x, request.pixel.pos.y);
                    }
                    PaintStatus::Cooldown => {
                        warn!("Token {} 仍在 CD 中", request.token_lease.uid());
                        request.token_lease.mark_failed();
                        
                        // 重试
                        if request.can_retry() {
                            request.increment_retry();
                            // TODO: 延迟重新入队
                        }
                    }
                    _ => {
                        warn!("绘制失败: {:?}", paint_result.status);
                        request.token_lease.mark_failed();
                    }
                }
            }
            Err(e) => {
                error!("绘制错误: {:?}", e);
                request.token_lease.mark_failed();
            }
        }
        
        // 避免过于频繁
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
```

**测试用例**:
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_paint_executor_success() { }
    
    #[tokio::test]
    async fn test_paint_executor_cooldown_retry() { }
}
```

---

### 👷 阶段 4: 重构 Worker（1 天）

#### 步骤 4.1: 简化 [`token_worker.rs`](src/app/multi_token/token_worker.rs)

**变更**: Worker 只负责组装请求，不直接绘制

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use log::{debug, error};

use super::token_manager::TokenManager;
use super::pixel_queue::PixelQueue;
use super::paint_executor::PaintRequestQueue;
use super::paint_request::PaintRequest;

pub struct TokenWorker {
    worker_id: usize,
    token_manager: Arc<TokenManager>,
    pixel_queue: Arc<PixelQueue>,
    request_queue: Arc<PaintRequestQueue>,
}

impl TokenWorker {
    pub fn new(
        worker_id: usize,
        token_manager: Arc<TokenManager>,
        pixel_queue: Arc<PixelQueue>,
        request_queue: Arc<PaintRequestQueue>,
    ) -> Self {
        Self {
            worker_id,
            token_manager,
            pixel_queue,
            request_queue,
        }
    }
    
    pub async fn run(&self, stop_signal: Arc<AtomicBool>) {
        debug!("TokenWorker {} 启动", self.worker_id);
        
        loop {
            if stop_signal.load(Ordering::Relaxed) {
                break;
            }
            
            // 1. 尝试获取 Token（非阻塞）
            let token_lease = match self.token_manager.try_acquire() {
                Some(lease) => lease,
                None => {
                    // 等待最短 CD
                    if let Some(wait) = self.token_manager.next_available_in() {
                        let wait = wait.min(Duration::from_millis(100));
                        tokio::time::sleep(wait).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    continue;
                }
            };
            
            // 2. 获取像素任务
            let pixel = match self.pixel_queue.try_pop().await {
                Some(p) => p,
                None => {
                    // 没有任务，释放 Token
                    token_lease.mark_failed();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };
            
            // 3. 组装并发送绘制请求
            let request = PaintRequest::new(pixel, token_lease);
            if let Err(e) = self.request_queue.send(request) {
                error!("Worker {}: 发送请求失败: {}", self.worker_id, e);
            }
        }
        
        debug!("TokenWorker {} 停止", self.worker_id);
    }
}
```

---

### 🔄 阶段 5: 优化 PixelQueue（1 天）

#### 步骤 5.1: 增量更新 [`pixel_queue.rs`](src/app/multi_token/pixel_queue.rs)

**添加方法**:

```rust
use std::collections::{BinaryHeap, HashMap};
use tokio::sync::Mutex;
use std::sync::Arc;
use winter_paintboard_sdk::models::Pos;

impl PixelQueue {
    /// 增量合并更新（而非清空）
    pub async fn merge_updates(&self, new_pixels: Vec<PriorityPixel>) {
        let mut queue = self.queue.lock().await;
        
        // 转换为 HashMap
        let mut pixel_map: HashMap<Pos, PriorityPixel> = queue
            .drain()
            .map(|p| (p.pos, p))
            .collect();
        
        // 合并新像素
        for pixel in new_pixels {
            pixel_map.insert(pixel.pos, pixel);
        }
        
        // 重建堆
        *queue = pixel_map.into_values().collect();
    }
}
```

---

### 🏗️ 阶段 6: 重构 MultiTokenService（1.5 天）

#### 步骤 6.1: 更新 [`multi_token_service.rs`](src/app/multi_token/multi_token_service.rs)

**主要变更**:
1. 创建 [`PaintExecutor`](PaintExecutor) 单线程
2. Worker 数量可配置
3. 使用 `RwLock` 优化 board 访问

```rust
use tokio::sync::RwLock;  // 改用 RwLock

pub struct MultiTokenService {
    workers: Vec<tokio::task::JoinHandle<()>>,
    executor_handle: Option<tokio::task::JoinHandle<()>>,  // NEW
    pixel_queue: Arc<PixelQueue>,
    local_board: Arc<RwLock<LocalBoard>>,  // 改用 RwLock
    // ...
    paint_request_queue: Arc<PaintRequestQueue>,  // NEW
}

impl MultiTokenService {
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 1. 启动 PaintExecutor
        let executor = PaintExecutor::new(
            self.shared_client.clone(),
            self.paint_request_queue.clone(),
            self.local_board.clone(),
        );
        
        let stop_signal = self.stop_signal.clone();
        self.executor_handle = Some(tokio::spawn(async move {
            executor.run(stop_signal).await;
        }));
        
        // 2. 启动 Workers
        for i in 0..self.worker_count {
            let worker = TokenWorker::new(
                i,
                self.token_manager.clone(),
                self.pixel_queue.clone(),
                self.paint_request_queue.clone(),
            );
            
            let stop_signal = self.stop_signal.clone();
            let handle = tokio::spawn(async move {
                worker.run(stop_signal).await;
            });
            
            self.workers.push(handle);
        }
        
        // 3. 启动比对循环（使用 read lock）
        // ...
        
        Ok(())
    }
    
    /// 优化的比对循环
    async fn run_comparison_loop(...) {
        // 使用 read() 而非 lock()
        let local_pixels = {
            let board = local_board.read().await;
            board.get_pixels().clone()
        };
        
        // 增量更新
        pixel_queue.merge_updates(differences).await;
    }
}
```

---

### ✅ 阶段 7: 测试和验证（2-3 天）

#### 步骤 7.1: 单元测试

```bash
# 运行所有测试
cargo test --package winter-paintboard-helper --lib app::multi_token

# 使用 loom 进行并发测试
cargo test --features loom
```

**测试清单**:
- [ ] [`TokenManager::try_acquire`](TokenManager::try_acquire) 并发安全性
- [ ] [`TokenLease`](TokenLease) RAII 自动释放
- [ ] [`PaintExecutor`](PaintExecutor) 串行化处理
- [ ] CD 时间精确性
- [ ] 队列增量更新

---

#### 步骤 7.2: 集成测试

```rust
#[tokio::test]
async fn test_multi_token_end_to_end() {
    // 模拟完整流程
}
```

---

#### 步骤 7.3: 压力测试

```bash
# 10 个 Token，1000 个像素
cargo run --release -- multi-token \
  --token-config tokens.json \
  --image test.png \
  --worker-count 10
```

**监控指标**:
- Token 可用率
- 绘制成功率
- 平均延迟
- CPU/内存使用

---

## 三、回滚计划

### 紧急回滚

如果重构导致严重问题，可立即回滚：

```bash
git revert <commit-hash>
```

### 分阶段发布

建议使用 feature flag:

```rust
#[cfg(feature = "new-multi-token")]
mod new_multi_token;

#[cfg(not(feature = "new-multi-token"))]
mod multi_token;
```

---

## 四、风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| [`paint_with_token`](paint_with_token) 性能问题 | 中 | 中 | 需修改 SDK 添加无状态接口 |
| 测试覆盖不足 | 低 | 高 | 使用 loom + 压力测试 |
| 引入新 bug | 低 | 高 | 分阶段发布 + feature flag |

---

## 五、成功标准

- ✅ 所有并发问题解决
- ✅ CD 时间精确度 < 50ms
- ✅ 测试覆盖率 > 80%
- ✅ 绘制成功率 > 95%
- ✅ 无死锁/数据竞争
- ✅ 内存使用下降 30%+

---

## 六、后续优化

完成基础重构后，可进一步优化：

1. **SDK 改进**: 添加真正的无状态绘制接口
2. **监控系统**: 集成 Prometheus metrics
3. **自适应 CD**: 根据服务端响应动态调整
4. **批量绘制**: 在允许的情况下使用 batch API