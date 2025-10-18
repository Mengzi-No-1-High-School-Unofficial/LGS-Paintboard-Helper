# 多 Token CD 机制设计方案

## 1. 概述

针对绘板组织引入的 Token CD 机制，设计新的 `multi-token` 模式，支持：
- 多 Token 并发绘制
- CD 时间管理
- 优先级队列
- Producer-Consumer 架构
- 与增量模式并行运行

## 2. 配置文件格式设计

### 2.1 JSON 配置文件结构

```json
{
  "cd_time_ms": 30000,
  "tokens": [
    {
      "uid": 123456,
      "access_key": "your-access-key-1"
    },
    {
      "uid": 789012,
      "access_key": "your-access-key-2"
    },
    {
      "uid": 345678,
      "token": "pre-fetched-token-uuid"
    }
  ]
}
```

### 2.2 配置项说明

- `cd_time_ms`: CD 时间（毫秒），默认 30000（30秒）
- `tokens`: Token 列表，每个 Token 可以是：
  - `uid` + `access_key`：运行时自动获取 token
  - `uid` + `token`：直接使用预获取的 token

## 3. 核心数据结构设计

### 3.1 TokenInfo - Token 信息

```rust
/// Token 信息和状态
pub struct TokenInfo {
    pub uid: u32,
    pub token: String,
    pub last_paint_time: Option<Instant>,
    pub is_available: bool,
}

impl TokenInfo {
    /// 检查 Token 是否可用（不在 CD 中）
    pub fn is_ready(&self, cd_duration: Duration) -> bool {
        match self.last_paint_time {
            None => true,
            Some(last_time) => last_time.elapsed() >= cd_duration,
        }
    }
    
    /// 标记 Token 已使用
    pub fn mark_used(&mut self) {
        self.last_paint_time = Some(Instant::now());
    }
}
```

### 3.2 TokenManager - Token 管理器

```rust
/// Token 管理器，负责管理多个 Token 的状态和分配
pub struct TokenManager {
    tokens: Vec<TokenInfo>,
    cd_duration: Duration,
}

impl TokenManager {
    /// 创建新的 TokenManager
    pub fn new(tokens: Vec<TokenInfo>, cd_duration: Duration) -> Self;
    
    /// 获取一个可用的 Token（非阻塞）
    pub fn try_acquire_token(&mut self) -> Option<&mut TokenInfo>;
    
    /// 异步等待获取可用 Token
    pub async fn acquire_token(&mut self) -> &mut TokenInfo;
    
    /// 获取下一个 Token 可用的时间
    pub fn next_available_time(&self) -> Option<Duration>;
}
```

### 3.3 PriorityPixel - 优先级像素

```rust
/// 带优先级的像素点
#[derive(Debug, Clone)]
pub struct PriorityPixel {
    pub pos: Pos,
    pub color: Rgb,
    pub priority: f64,  // 颜色差异值，越大越优先
}

impl Ord for PriorityPixel {
    fn cmp(&self, other: &Self) -> Ordering {
        // 优先级高的排在前面（大顶堆）
        other.priority.partial_cmp(&self.priority).unwrap_or(Ordering::Equal)
    }
}
```

### 3.4 PixelQueue - 优先级队列

```rust
/// 线程安全的优先级像素队列
pub struct PixelQueue {
    queue: Arc<Mutex<BinaryHeap<PriorityPixel>>>,
    version: Arc<AtomicU64>,  // 队列版本号，用于检测更新
}

impl PixelQueue {
    pub fn new() -> Self;
    
    /// 清空队列并添加新像素（用于新的比对）
    pub async fn reset_and_push(&self, pixels: Vec<PriorityPixel>);
    
    /// 尝试弹出一个像素（非阻塞）
    pub async fn try_pop(&self) -> Option<PriorityPixel>;
    
    /// 获取队列长度
    pub async fn len(&self) -> usize;
    
    /// 获取当前版本号
    pub fn version(&self) -> u64;
}
```

## 4. Worker 架构设计

### 4.1 TokenWorker - Token 工作器

```rust
/// Token 工作器，负责从队列中取像素并绘制
pub struct TokenWorker {
    token_info: TokenInfo,
    cd_duration: Duration,
    client: Box<dyn PaintboardClientTrait + Send>,
}

impl TokenWorker {
    /// 创建新的 Worker
    pub async fn new(
        uid: u32,
        token: String,
        cd_duration: Duration,
        ws_url: String,
    ) -> Result<Self, PaintboardError>;
    
    /// 启动 Worker 主循环
    pub async fn run(
        mut self,
        pixel_queue: Arc<PixelQueue>,
        local_board: Arc<Mutex<LocalBoard>>,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>>;
    
    /// 使用指定 Token 绘制像素
    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
    ) -> Result<PaintResult, PaintboardError>;
}
```

Worker 工作流程：
```
loop {
    if stop_signal.is_set() { break; }
    
    // 检查是否可以绘制（不在 CD 中）
    if !self.token_info.is_ready(cd_duration) {
        sleep_until_ready();
        continue;
    }
    
    // 尝试从队列获取像素
    if let Some(pixel) = pixel_queue.try_pop().await {
        // 绘制像素
        match self.paint_with_token(pixel.pos, pixel.color).await {
            Ok(result) if result.status == Success => {
                // 更新本地绘板
                local_board.update_pixel(pos.x, pos.y, color, PixelSource::Own);
                // 标记 Token 已使用
                self.token_info.mark_used();
            }
            Ok(result) => {
                // 处理其他状态（如 Cooldown）
                warn!("绘制失败: {:?}", result.status);
            }
            Err(e) => {
                // 处理错误
                error!("绘制错误: {:?}", e);
            }
        }
    } else {
        // 队列为空，等待
        sleep(Duration::from_millis(100));
    }
}
```

## 5. 主服务协调器设计

### 5.1 MultiTokenService

```rust
/// 多 Token 绘制服务
pub struct MultiTokenService {
    workers: Vec<JoinHandle<()>>,
    pixel_queue: Arc<PixelQueue>,
    local_board: Arc<Mutex<LocalBoard>>,
    target_image: ProcessedImageData,
    start_x: i32,
    start_y: i32,
    stop_signal: Arc<AtomicBool>,
    comparison_interval: Duration,
}

impl MultiTokenService {
    /// 创建新服务
    pub fn new(
        token_config: TokenConfig,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        comparison_interval: Duration,
    ) -> Self;
    
    /// 启动服务
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    
    /// 停止服务
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    
    /// 比对循环（Producer）
    async fn comparison_loop(&self);
}
```

### 5.2 服务启动流程

```rust
impl MultiTokenService {
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("启动多 Token 绘制服务...");
        
        // 1. 为每个 Token 创建 Worker
        for token_info in &token_config.tokens {
            let worker = TokenWorker::new(
                token_info.uid,
                token_info.token.clone(),
                self.token_config.cd_duration,
                self.token_config.ws_url.clone(),
            ).await?;
            
            let queue = self.pixel_queue.clone();
            let board = self.local_board.clone();
            let stop = self.stop_signal.clone();
            
            // 启动 Worker
            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run(queue, board, stop).await {
                    error!("Worker 运行错误: {:?}", e);
                }
            });
            
            self.workers.push(handle);
        }
        
        // 2. 启动比对循环
        let queue = self.pixel_queue.clone();
        let board = self.local_board.clone();
        let target = self.target_image.clone();
        let start_x = self.start_x;
        let start_y = self.start_y;
        let interval = self.comparison_interval;
        let stop = self.stop_signal.clone();
        
        tokio::spawn(async move {
            Self::run_comparison_loop(
                queue, board, target, start_x, start_y, interval, stop
            ).await;
        });
        
        info!("多 Token 服务已启动，Worker 数量: {}", self.workers.len());
        Ok(())
    }
}
```

### 5.3 比对循环实现

```rust
impl MultiTokenService {
    async fn run_comparison_loop(
        pixel_queue: Arc<PixelQueue>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        interval: Duration,
        stop_signal: Arc<AtomicBool>,
    ) {
        let mut interval_timer = tokio::time::interval(interval);
        
        loop {
            if stop_signal.load(Ordering::Relaxed) {
                break;
            }
            
            interval_timer.tick().await;
            
            info!("开始比对绘版与目标图片...");
            
            // 获取本地绘版数据
            let local_pixels = {
                let board = local_board.lock().await;
                if !board.is_initialized() {
                    warn!("本地绘版未初始化，跳过比对");
                    continue;
                }
                board.get_pixels().clone()
            };
            
            // 计算差异像素
            let mut differences = Vec::new();
            for (pos, target_color) in &target_image.full_scale_operations {
                let x = pos.x as i32;
                let y = pos.y as i32;
                
                let relative_x = x - start_x;
                let relative_y = y - start_y;
                
                if relative_x >= 0 && relative_y >= 0
                    && relative_x < target_image.img_width as i32
                    && relative_y < target_image.img_height as i32
                {
                    let color_diff = if let Some(current_pixel) = local_pixels.get(pos) {
                        if current_pixel.color != *target_color {
                            calculate_color_difference(&current_pixel.color, target_color)
                        } else {
                            continue; // 颜色一致，跳过
                        }
                    } else {
                        255.0 // 缺少像素，最高优先级
                    };
                    
                    differences.push(PriorityPixel {
                        pos: *pos,
                        color: *target_color,
                        priority: color_diff,
                    });
                }
            }
            
            // 更新队列（清空并添加新数据）
            if !differences.is_empty() {
                info!("检测到 {} 个像素差异，更新队列", differences.len());
                pixel_queue.reset_and_push(differences).await;
            } else {
                info!("未检测到像素差异");
            }
        }
    }
}
```

## 6. SDK 扩展设计

### 6.1 新的 paint 函数

在 [`paintboard_client_trait.rs`](crates/winter-paintboard-sdk/src/client/paintboard_client_trait.rs:1) 中添加：

```rust
pub trait PaintboardClientTrait {
    // ... 现有方法 ...
    
    /// 使用临时 Token 绘制像素（不修改客户端状态）
    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<PaintResult, PaintboardError>;
}
```

在 WebSocket 客户端实现中：

```rust
impl PaintboardClientTrait for BasicClient {
    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<PaintResult, PaintboardError> {
        // 创建临时的 PaintOperation
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token: token.clone(),
            paint_id: self.next_paint_id(),
        };
        
        // 发送绘制请求
        self.send_paint_operation(operation).await
    }
}
```

## 7. CLI 命令行扩展

### 7.1 新命令设计

在 [`cli.rs`](src/app/cli.rs:1) 中添加：

```rust
#[derive(Subcommand)]
pub enum Commands {
    // ... 现有命令 ...
    
    /// 多 Token CD 模式：使用多个 Token 并发绘制
    MultiToken {
        /// Token 配置文件路径（JSON 格式）
        #[arg(short, long)]
        config: PathBuf,
        
        /// 或者直接通过命令行提供 access keys（逗号分隔）
        #[arg(long, conflicts_with = "config")]
        access_keys: Option<String>,
        
        /// 对应的 UIDs（逗号分隔，与 access_keys 对应）
        #[arg(long, requires = "access_keys")]
        uids: Option<String>,
        
        /// CD 时间（毫秒）
        #[arg(long, default_value_t = 30000)]
        cd_time: u64,
        
        /// WebSocket 端点 URL
        #[arg(long)]
        ws_url: Option<String>,
        
        /// 要绘制的图片路径
        #[arg(short, long)]
        image: PathBuf,
        
        /// 起始 X 坐标
        #[arg(short, long, default_value_t = 0)]
        x: i32,
        
        /// 起始 Y 坐标
        #[arg(short, long, default_value_t = 0)]
        y: i32,
        
        /// 图片宽度
        #[arg(long)]
        width: Option<u32>,
        
        /// 图片高度
        #[arg(long)]
        height: Option<u32>,
        
        /// 比对间隔（毫秒）
        #[arg(long, default_value_t = 5000)]
        comparison_interval: u64,
    },
}
```

### 7.2 配置加载逻辑

```rust
/// Token 配置
#[derive(Debug, Clone, Deserialize)]
pub struct TokenConfig {
    pub cd_time_ms: u64,
    pub tokens: Vec<TokenEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenEntry {
    pub uid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl TokenConfig {
    /// 从文件加载配置
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: TokenConfig = serde_json::from_str(&content)?;
        Ok(config)
    }
    
    /// 从命令行参数构建配置
    pub fn from_cli_args(
        access_keys: String,
        uids: String,
        cd_time: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let keys: Vec<&str> = access_keys.split(',').collect();
        let uids: Vec<u32> = uids
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<Vec<_>, _>>()?;
        
        if keys.len() != uids.len() {
            return Err("access_keys 和 uids 数量不匹配".into());
        }
        
        let tokens = keys
            .into_iter()
            .zip(uids.into_iter())
            .map(|(key, uid)| TokenEntry {
                uid,
                access_key: Some(key.to_string()),
                token: None,
            })
            .collect();
        
        Ok(TokenConfig {
            cd_time_ms: cd_time,
            tokens,
        })
    }
}
```

## 8. 文件结构规划

```
src/app/multi_token/
├── mod.rs                    # 模块导出
├── config.rs                 # TokenConfig, TokenEntry
├── token_manager.rs          # TokenInfo, TokenManager
├── pixel_queue.rs            # PriorityPixel, PixelQueue
├── token_worker.rs           # TokenWorker
├── multi_token_service.rs    # MultiTokenService
└── comparison.rs             # 比对逻辑（复用增量模式的代码）
```

## 9. 集成流程

### 9.1 在 main.rs 中添加新模式

```rust
async fn run_multi_token_mode(
    config: Option<PathBuf>,
    access_keys: Option<String>,
    uids: Option<String>,
    cd_time: u64,
    ws_url: Option<String>,
    image: PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    comparison_interval: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 加载配置
    let token_config = if let Some(config_path) = config {
        TokenConfig::from_file(&config_path)?
    } else if let (Some(keys), Some(ids)) = (access_keys, uids) {
        TokenConfig::from_cli_args(keys, ids, cd_time)?
    } else {
        return Err("必须提供配置文件或命令行参数".into());
    };
    
    // 2. 解析所有 Token（将 access_key 转换为 token）
    let mut resolved_tokens = Vec::new();
    for entry in &token_config.tokens {
        let token = if let Some(token) = &entry.token {
            token.clone()
        } else if let Some(access_key) = &entry.access_key {
            get_token_with_access_key(entry.uid, access_key).await?
        } else {
            return Err(format!("Token entry for UID {} 缺少 token 或 access_key", entry.uid).into());
        };
        
        resolved_tokens.push(TokenInfo {
            uid: entry.uid,
            token,
            last_paint_time: None,
            is_available: true,
        });
    }
    
    // 3. 创建同步管理器（复用增量模式的逻辑）
    let event_bus = winter_paintboard_sdk::event::EventBus::global();
    let sync_manager = BoardSyncManager::new(&event_bus);
    
    // 4. 启动同步服务
    let mut sync_config = Config::default();
    if let Some(url) = ws_url.clone() {
        sync_config.ws_url = url;
    }
    let mut sync_client = BasicClient::new(sync_config).await?;
    sync_client.set_auth(resolved_tokens[0].uid, resolved_tokens[0].token.clone());
    
    sync_manager.start_incremental_sync_loop(
        Box::new(sync_client),
        Duration::from_millis(comparison_interval),
    ).await?;
    
    sync_manager.start_event_listener().await?;
    
    // 5. 处理图片
    let processed_image = process_image_at_all_scales(&image, width, height, x, y)?;
    
    // 6. 创建并启动 MultiTokenService
    let mut service = MultiTokenService::new(
        resolved_tokens,
        Duration::from_millis(token_config.cd_time_ms),
        ws_url.unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string()),
        sync_manager.local_board(),
        processed_image,
        x,
        y,
        Duration::from_millis(comparison_interval),
    );
    
    service.start().await?;
    
    // 7. 等待中断信号
    info!("多 Token 模式已启动，按 Ctrl+C 停止...");
    tokio::signal::ctrl_c().await?;
    
    // 8. 清理
    service.stop().await?;
    Ok(())
}
```

## 10. 与增量模式的共存

两种模式可以独立运行：

- **增量模式**：使用单个 Token，支持初始绘制 + 监控修复
- **多 Token 模式**：使用多个 Token，无初始绘制，仅持续比对和修复

用户可以根据需求选择：
- 如果没有 CD 限制或 CD 时间短：使用增量模式
- 如果有严格的 Token CD 限制：使用多 Token 模式

## 11. 关键实现细节

### 11.1 LocalBoard 手动更新

在 Worker 成功绘制后：

```rust
// 绘制成功后手动更新 LocalBoard
if result.status == PaintStatus::Success {
    let mut board = local_board.lock().await;
    board.update_pixel(
        pixel.pos.x,
        pixel.pos.y,
        pixel.color,
        PixelSource::Own,
    );
}
```

### 11.2 并发安全

- `PixelQueue` 使用 `Arc<Mutex<BinaryHeap>>` 保证线程安全
- `LocalBoard` 已经是 `Arc<Mutex<LocalBoard>>`，可以安全共享
- 每个 Worker 独立运行，不共享客户端连接

### 11.3 性能优化

- 使用 `BinaryHeap` 实现 O(log n) 的优先级队列
- Worker 使用非阻塞的 `try_pop`，避免等待锁
- 比对循环可配置间隔，避免过度消耗 CPU

## 12. 测试策略

1. **单元测试**：
   - TokenManager 的 Token 分配逻辑
   - PixelQueue 的优先级排序
   - TokenInfo 的 CD 计算

2. **集成测试**：
   - 多 Worker 并发绘制
   - 队列清空和更新机制
   - LocalBoard 更新正确性

3. **端到端测试**：
   - 使用测试服务器验证完整流程
   - 验证 CD 机制是否正确工作

## 13. 后续扩展

1. **动态 Token 管理**：支持运行时添加/删除 Token
2. **统计和监控**：绘制速率、队列长度、Worker 状态
3. **失败重试**：对失败的像素进行重试
4. **配置热重载**：支持修改 CD 时间而不重启程序

---

## 总结

本设计方案提供了完整的多 Token CD 模式实现路径，包括：
- ✅ 清晰的数据结构设计
- ✅ Producer-Consumer 架构
- ✅ 优先级队列机制
- ✅ CLI 集成方案
- ✅ 与现有代码的兼容性

下一步可以切换到 Code 模式开始实现。