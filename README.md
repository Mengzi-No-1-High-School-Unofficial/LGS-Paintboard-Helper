# 冬日绘板图片绘制工具

这是一个使用 Rust 编写的命令行工具，可以将 PNG 图片绘制到冬日绘板上。

> [!warning]
>
> 本程序中包含人工智能生成的代码，其中部分代码尚未经过完全的人工审查，请谨慎使用

> [!note]
>
> 该程序中的一切人工智能生成代码，必须在未来经过完全的人工重构后使用！

## 功能特性
- 支持从 PNG 文件读取图片（包括 Alpha 通道）
- 允许用户指定认证 Token 和 UID
- 支持设置图片在画板上的位置 (X, Y 坐标)
- 支持设置图片绘制时的尺寸（宽度和高度）
- 自动跳过透明像素 (Alpha 值为 0)
- 自动验证画板坐标边界

## 安装

确保您已安装 Rust 和 Cargo。

1. 克隆或下载项目代码
2. 在项目根目录运行：

```bash
cargo build --release
```

生成的可执行文件在 `target/release/lgs-paintboard`。

## 使用方法

```bash
lgs-paintboard --token <TOKEN> --uid <UID> --image <IMAGE_PATH> [OPTIONS]
```

### 参数说明

- `--token <TOKEN>` 或 `--access-key <ACCESS_KEY>`: 
  - 直接提供认证 Token，或提供访问密钥让程序自动获取 Token [二选一]
- `--uid <UID>`: 用户 ID [必需]
- `--image <IMAGE_PATH>`: 要绘制的 PNG 图片路径 [必需]

### 可选参数

- `--ws_url <WS_URL>`: WebSocket 端点 URL (默认为官方端点)
- `--delay <DELAY>`: 绘制每个像素之间的延迟（毫秒）(默认为10毫秒，减少此值会增加发送速率)  
- `--batch-mode <BATCH_MODE>`: 是否使用批量绘制模式（粘包发送）(默认为 true)
- `--max-batch-size <MAX_BATCH_SIZE>`: 批量模式下每次发送的最大像素数量 (默认为50)  
- `-x <X>`: 图片在画板上的起始 X 坐标 (默认为 0)
- `-y <Y>`: 图片在画板上的起始 Y 坐标 (默认为 0)
- `--width <WIDTH>`: 图片绘制时的宽度 (默认为原图高度)
- `--height <HEIGHT>`: 图片绘制时的高度 (默认为原图高度)

## Multi Token 模式（高效并发绘制）

多 Token 模式能够充分利用多个账户的 Token，实现真正的并发绘制，大幅提升绘制效率。

### 工作原理

Multi Token 系统采用以下架构：

- **TokenManager**: 管理所有 Token 的状态（可用/已获取/冷却中）
- **PixelQueue**: 按颜色差异排序的优先级队列
- **TokenWorker**: N 个并发 Worker，每个获取一个 Token 并从队列获取像素任务
- **PaintExecutor**: 单线程执行器，通过共享客户端串行执行绘制（避免触发 API 并发限制）
- **比对循环**: 定期比对本地绘版与目标图片，发现差异像素并入队

### 配置方式

#### 方式 1：JSON 配置文件

创建 `tokens.json`（或其他名称）：

```json
{
  "cd_time_ms": 30000,
  "tokens": [
    { "uid": 12345, "token": "your_token_1" },
    { "uid": 12346, "token": "your_token_2" },
    { "uid": 12347, "token": "your_token_3" }
  ]
}
```

**参数说明**：
- `cd_time_ms`: Token 冷却时间（毫秒），API 通常为 30000ms
- `uid`: 用户 ID
- `token`: 认证 Token 字符串

然后使用：

```bash
lgs-paintboard multi-token --config tokens.json --image image.png -x 0 -y 0
```

#### 方式 2：命令行参数

```bash
lgs-paintboard multi-token \
  --uids 12345,12346,12347 \
  --tokens "token1,token2,token3" \
  --cd-time 30000 \
  --image image.png \
  -x 0 -y 0
```

### 高级选项

```bash
lgs-paintboard multi-token \
  --config tokens.json \
  --image image.png \
  -x 0 -y 0 \
  --comparison-interval 30000 \
  --ws-url ws://your_ws_url \
  --log-level info
```

- `--comparison-interval <MS>`: 比对循环间隔（毫秒，默认 30000）
- `--ws-url <URL>`: WebSocket 端点 URL
- `--log-level <LEVEL>`: 日志级别（debug/info/warn/error）

### 使用示例

#### 示例 1：3 个 Token 绘制图片

```bash
# 创建 tokens.json
cat > tokens.json << EOF
{
  "cd_time_ms": 30000,
  "tokens": [
    { "uid": 123456, "token": "xxxxxxxx_token_1" },
    { "uid": 123457, "token": "xxxxxxxx_token_2" },
    { "uid": 123458, "token": "xxxxxxxx_token_3" }
  ]
}
EOF

# 开始绘制
lgs-paintboard multi-token --config tokens.json --image target.png -x 100 -y 50
```

#### 示例 2：命令行模式（快速测试）

```bash
lgs-paintboard multi-token \
  --uids 123456,123457,123458 \
  --tokens "token1,token2,token3" \
  --cd-time 30000 \
  --image target.png \
  -x 0 -y 0
```

### 性能对比

| 配置 | Token 数 | CD 时间 | 单个像素耗时 | 理论吞吐量 |
|-----|---------|--------|----------|---------|
| 单 Token | 1 | 30s | 30s | 1 px/30s |
| 双 Token | 2 | 30s | 15s | 2 px/30s |
| 三 Token | 3 | 30s | 10s | 3 px/30s |
| N Token | N | 30s | 30/N s | N px/30s |

**实际例子**: 1000 个像素
- 单 Token: 1000 × 30s = 8.3 小时
- 3 Token: 1000 ÷ 3 ≈ 334 × 10s = 55 分钟
- **加速 ~9 倍！**

### 工作流程

1. **初始化**：加载 Token，创建队列和 Worker
2. **比对**：定期比对本地绘版与目标图片
3. **入队**：发现的差异像素按优先级（颜色差异）入队
4. **并发获取**：N 个 Worker 并发获取 Token 和像素任务
5. **串行执行**：单个 Executor 通过共享客户端执行绘制
6. **结果处理**：
   - 成功 → Token 进入 CD，本地绘版更新
   - CD 冲突 → 释放 Token，像素延迟重新入队（指数退避）
   - 网络错误 → 释放 Token，像素延迟重新入队

### 重试机制

Multi Token 系统会自动处理失败和重试：

- **CD 冲突**: Token 正在冷却中
  - 行动: 释放 Token，像素重新入队
  - 延迟: 100ms × 2^retry_count（指数退避）
  
- **网络错误**: 连接失败或超时
  - 行动: 释放 Token，像素重新入队
  - 延迟: 300ms × 2^retry_count

- **其他错误**: API 返回错误状态
  - 行动: 释放 Token，像素重新入队
  - 延迟: 200ms × 2^retry_count

最多重试 3 次，之后放弃该像素。

### 常见问题

**Q: 为什么不用多个客户端并发绘制？**
A: 洛谷 API 对并发连接有限制，多客户端会触发限流。单 Executor 设计避免这个问题。

**Q: 如何选择 Token 数量？**
A: 一般 3-5 个 Token 为最佳平衡。过多会增加管理复杂度，但边际效益递减。

**Q: 为什么像素重新入队后会被重新处理？**
A: 这是故意的设计。失败的像素会保留优先级，被其他可用的 Token 继续处理，提升最终成功率。

### 故障排查

**日志输出示例**：

```
2025-10-19 08:30:00 INFO: 启动多 Token 绘制服务，Token 数量: 3
2025-10-19 08:30:00 INFO: Worker 0 已启动
2025-10-19 08:30:00 INFO: Worker 1 已启动
2025-10-19 08:30:00 INFO: Worker 2 已启动
2025-10-19 08:30:30 INFO: 检测到 125 个像素差异，更新队列
2025-10-19 08:30:31 INFO: 绘制成功: (100, 50)
2025-10-19 08:31:02 WARN: CD 冲突，重试绘制请求 1/3: (101, 51)
```


### 示例

```bash
# 方式1：直接使用 Token
lgs-paintboard --token YOUR_TOKEN_HERE --uid 12345 --image path/to/your/image.png

# 方式2：使用 UID 和访问密钥让程序自动获取 Token
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image path/to/your/image.png

# 指定位置
lgs-paintboard --token YOUR_TOKEN_HERE --uid 12345 --image image.png -x 10 -y 20

# 指定位置和尺寸
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image image.png -x 10 -y 20 --width 100 --height 50

# 指定较小的延迟以提高绘制速度（注意：可能触发速率限制）
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image image.png --delay 5

# 指定较大的延迟以降低绘制速度（更安全）
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image image.png --delay 50

# 使用较小的批量大小（更安全，但速度较慢）
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image image.png --max-batch-size 10

# 禁用批量模式，逐个发送像素（最安全，但速度最慢）
lgs-paintboard --uid 12345 --access-key YOUR_ACCESS_KEY_HERE --image image.png --batch-mode false
```

## 注意事项

- X 坐标范围：0-999
- Y 坐标范围：0-599
- 透明区域（Alpha 值为 0）的像素不会被绘制
- 位于画板边界外的像素将被忽略并显示警告