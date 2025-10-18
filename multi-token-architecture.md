# 多 Token CD 模式架构图

## 1. 整体架构图

```mermaid
graph TB
    subgraph CLI["命令行入口"]
        User[用户] -->|multi-token 命令| CLI_Parser[CLI Parser]
        CLI_Parser -->|加载配置| Config[TokenConfig]
    end
    
    subgraph Auth["认证层"]
        Config -->|access_key| TokenResolver[Token Resolver]
        TokenResolver -->|HTTP| API[Paintboard API]
        API -->|返回 token| TokenResolver
        TokenResolver -->|resolved tokens| TokenList[Token List]
    end
    
    subgraph Core["核心服务层"]
        TokenList --> Service[MultiTokenService]
        Service -->|创建| Workers[Token Workers Pool]
        Service -->|创建| Queue[Priority Pixel Queue]
        Service -->|启动| Comparator[Comparison Loop]
    end
    
    subgraph Sync["同步层"]
        SyncManager[Board Sync Manager] -->|WebSocket 事件| LocalBoard[Local Board]
        SyncManager -->|定期全量同步| LocalBoard
    end
    
    subgraph Worker["Worker 层"]
        Workers -->|包含| W1[Worker 1<br/>Token A]
        Workers -->|包含| W2[Worker 2<br/>Token B]
        Workers -->|包含| W3[Worker N<br/>Token N]
        
        W1 -.->|try_pop| Queue
        W2 -.->|try_pop| Queue
        W3 -.->|try_pop| Queue
        
        W1 -->|paint_with_token| WS1[WebSocket Conn 1]
        W2 -->|paint_with_token| WS2[WebSocket Conn 2]
        W3 -->|paint_with_token| WS3[WebSocket Conn N]
        
        WS1 --> Server[Paintboard Server]
        WS2 --> Server
        WS3 --> Server
        
        W1 -.->|更新| LocalBoard
        W2 -.->|更新| LocalBoard
        W3 -.->|更新| LocalBoard
    end
    
    subgraph Producer["比对生产者"]
        Comparator -->|读取| LocalBoard
        Comparator -->|读取| TargetImage[Target Image]
        Comparator -->|计算差异| Diff[Pixel Differences]
        Diff -->|按优先级排序| Queue
    end
    
    style Service fill:#f9f,stroke:#333,stroke-width:4px
    style Queue fill:#bbf,stroke:#333,stroke-width:2px
    style LocalBoard fill:#bfb,stroke:#333,stroke-width:2px
    style Server fill:#fbb,stroke:#333,stroke-width:2px
```

## 2. 数据流图

```mermaid
sequenceDiagram
    participant U as 用户
    participant CLI as CLI
    participant S as MultiTokenService
    participant C as Comparator Loop
    participant Q as Pixel Queue
    participant W1 as Worker 1
    participant W2 as Worker 2
    participant LB as LocalBoard
    participant PS as Paintboard Server
    
    U->>CLI: 启动 multi-token 命令
    CLI->>S: 创建服务（配置、tokens）
    S->>W1: 创建 Worker（Token A）
    S->>W2: 创建 Worker（Token B）
    S->>C: 启动比对循环
    
    par 比对循环
        loop 每隔 comparison_interval
            C->>LB: 获取当前绘版状态
            C->>C: 比对与目标图片差异
            C->>Q: 清空队列并推入差异像素（按优先级）
        end
    and Worker 1 循环
        loop 持续运行
            W1->>W1: 检查是否在 CD 中
            alt 不在 CD
                W1->>Q: try_pop 获取像素
                alt 队列非空
                    W1->>PS: paint_with_token(pixel, Token A)
                    PS-->>W1: 返回结果
                    alt 成功
                        W1->>LB: 更新本地绘版
                        W1->>W1: 标记 Token 已使用（更新 CD）
                    end
                end
            else 在 CD 中
                W1->>W1: 等待 CD 结束
            end
        end
    and Worker 2 循环
        loop 持续运行
            W2->>W2: 检查是否在 CD 中
            alt 不在 CD
                W2->>Q: try_pop 获取像素
                alt 队列非空
                    W2->>PS: paint_with_token(pixel, Token B)
                    PS-->>W2: 返回结果
                    alt 成功
                        W2->>LB: 更新本地绘版
                        W2->>W2: 标记 Token 已使用（更新 CD）
                    end
                end
            end
        end
    end
```

## 3. 状态机图（Worker）

```mermaid
stateDiagram-v2
    [*] --> Idle: Worker 启动
    
    Idle --> CheckCD: 开始循环
    
    CheckCD --> WaitCD: Token 在 CD 中
    CheckCD --> TryPop: Token 可用
    
    WaitCD --> CheckCD: CD 结束
    
    TryPop --> Idle: 队列为空（等待）
    TryPop --> Paint: 获取到像素
    
    Paint --> Success: 绘制成功
    Paint --> Failed: 绘制失败
    
    Success --> UpdateBoard: 更新 LocalBoard
    UpdateBoard --> MarkCD: 标记 Token CD
    MarkCD --> CheckCD: 继续循环
    
    Failed --> CheckCD: 继续循环（不更新 CD）
    
    CheckCD --> [*]: 收到停止信号
```

## 4. 类图

```mermaid
classDiagram
    class TokenConfig {
        +u64 cd_time_ms
        +Vec~TokenEntry~ tokens
        +from_file(path: Path) TokenConfig
        +from_cli_args(keys, uids, cd) TokenConfig
    }
    
    class TokenEntry {
        +u32 uid
        +Option~String~ access_key
        +Option~String~ token
    }
    
    class TokenInfo {
        +u32 uid
        +String token
        +Option~Instant~ last_paint_time
        +bool is_available
        +is_ready(cd_duration: Duration) bool
        +mark_used()
    }
    
    class TokenManager {
        -Vec~TokenInfo~ tokens
        -Duration cd_duration
        +new(tokens, cd) TokenManager
        +try_acquire_token() Option~TokenInfo~
        +acquire_token() TokenInfo
        +next_available_time() Option~Duration~
    }
    
    class PriorityPixel {
        +Pos pos
        +Rgb color
        +f64 priority
    }
    
    class PixelQueue {
        -Arc~Mutex~BinaryHeap~PriorityPixel~~~ queue
        -Arc~AtomicU64~ version
        +new() PixelQueue
        +reset_and_push(pixels: Vec)
        +try_pop() Option~PriorityPixel~
        +len() usize
        +version() u64
    }
    
    class TokenWorker {
        -TokenInfo token_info
        -Duration cd_duration
        -Box~PaintboardClientTrait~ client
        +new(uid, token, cd, ws_url) TokenWorker
        +run(queue, board, stop) Result
        -paint_with_token(pos, color) Result
    }
    
    class MultiTokenService {
        -Vec~JoinHandle~ workers
        -Arc~PixelQueue~ pixel_queue
        -Arc~Mutex~LocalBoard~~ local_board
        -ProcessedImageData target_image
        -i32 start_x
        -i32 start_y
        -Arc~AtomicBool~ stop_signal
        -Duration comparison_interval
        +new(...) MultiTokenService
        +start() Result
        +stop() Result
        -run_comparison_loop(...)
    }
    
    class PaintboardClientTrait {
        <<interface>>
        +paint_with_token(pos, color, uid, token) Result~PaintResult~
    }
    
    TokenConfig "1" *-- "many" TokenEntry
    TokenManager "1" *-- "many" TokenInfo
    PixelQueue "1" *-- "many" PriorityPixel
    MultiTokenService "1" --> "1" PixelQueue
    MultiTokenService "1" --> "many" TokenWorker
    TokenWorker "1" --> "1" TokenInfo
    TokenWorker "1" --> "1" PaintboardClientTrait
    TokenWorker "1" --> "1" PixelQueue
    
    PriorityPixel ..|> Ord: implements
```

## 5. 组件交互时序图（详细）

```mermaid
sequenceDiagram
    participant Main as main.rs
    participant Svc as MultiTokenService
    participant Cmp as Comparator
    participant Q as PixelQueue
    participant W as TokenWorker
    participant TM as TokenManager
    participant TI as TokenInfo
    participant Client as BasicClient
    participant Server as Paintboard Server
    participant LB as LocalBoard
    
    Main->>Svc: new(config, tokens, ...)
    Svc->>Q: 创建优先级队列
    Svc->>TM: 创建 TokenManager
    
    Main->>Svc: start()
    
    loop 为每个 Token
        Svc->>W: 创建 Worker(token_info)
        W->>Client: 创建 WebSocket 客户端
        Svc->>W: spawn run() task
    end
    
    Svc->>Cmp: spawn comparison_loop()
    
    par Comparison Loop
        loop 每隔 interval
            Cmp->>LB: 获取当前像素状态
            Cmp->>Cmp: 计算与目标的差异
            Cmp->>Q: reset_and_push(differences)
            Note over Q: 清空旧数据<br/>按优先级排序新数据
        end
    and Worker Loop
        loop 持续运行
            W->>TI: is_ready(cd_duration)?
            alt Token 在 CD
                W->>W: sleep(until_ready)
            else Token 可用
                W->>Q: try_pop()
                alt 队列有数据
                    Q-->>W: PriorityPixel
                    W->>Client: paint_with_token(pos, color, uid, token)
                    Client->>Server: WebSocket: Paint Operation
                    Server-->>Client: Paint Result
                    Client-->>W: Result<PaintResult>
                    alt 绘制成功
                        W->>LB: update_pixel(x, y, color, Own)
                        W->>TI: mark_used()
                        Note over TI: 更新 last_paint_time<br/>开始新的 CD
                    else 绘制失败
                        W->>W: log error
                        Note over W: 不更新 CD<br/>下次可以重试
                    end
                else 队列为空
                    W->>W: sleep(100ms)
                end
            end
        end
    end
    
    Main->>Svc: ctrl+c signal
    Svc->>Svc: stop()
    Svc->>W: 发送停止信号
    W->>W: 退出循环
```

## 6. 部署架构图

```mermaid
graph LR
    subgraph User["用户环境"]
        Config[tokens.json<br/>配置文件]
        Image[target.png<br/>目标图片]
        Binary[paintboard-helper<br/>可执行文件]
    end
    
    subgraph Process["进程内部"]
        Main[Main Thread]
        
        subgraph Threads["Worker Threads"]
            T1[Worker Thread 1]
            T2[Worker Thread 2]
            TN[Worker Thread N]
        end
        
        subgraph Services["后台服务"]
            Sync[Sync Service]
            Compare[Comparison Service]
        end
        
        Memory[(Shared Memory<br/>LocalBoard<br/>PixelQueue)]
    end
    
    subgraph Network["网络层"]
        WS1[WebSocket 1]
        WS2[WebSocket 2]
        WSN[WebSocket N]
    end
    
    subgraph Server["洛谷服务器"]
        API[Paintboard API]
        Board[Canvas Board]
    end
    
    Config --> Binary
    Image --> Binary
    Binary --> Main
    Main --> Threads
    Main --> Services
    Threads --> Memory
    Services --> Memory
    
    T1 --> WS1
    T2 --> WS2
    TN --> WSN
    
    WS1 --> API
    WS2 --> API
    WSN --> API
    
    API --> Board
    
    style Memory fill:#bfb,stroke:#333,stroke-width:2px
    style Board fill:#fbb,stroke:#333,stroke-width:2px
```

---

## 设计要点总结

### 核心思想
1. **Producer-Consumer 模式**：比对循环作为生产者，Workers 作为消费者
2. **优先级调度**：使用二叉堆实现优先级队列，优先修复颜色差异大的像素
3. **CD 管理**：每个 Worker 独立管理自己的 Token CD 状态
4. **无锁设计**：Workers 使用 try_pop 避免阻塞，提高并发性能

### 关键优化
1. **队列版本号**：避免 Workers 处理过期数据
2. **非阻塞获取**：Workers 使用 try_pop 而不是阻塞等待
3. **本地更新**：绘制成功后立即更新 LocalBoard，避免重复绘制
4. **错误恢复**：绘制失败不更新 CD，允许下次重试

### 扩展性
1. **动态 Token**：TokenManager 可以支持运行时添加/删除 Token
2. **监控指标**：可以添加绘制速率、队列长度等统计
3. **配置热重载**：可以支持运行时调整 CD 时间