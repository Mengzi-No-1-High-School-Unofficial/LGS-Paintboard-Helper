# 监控系统 HTTP API 文档

## 概述

监控系统提供 HTTP API 用于查询 Worker 的运行状态和绘制性能数据。Master 节点在启动时会启动 HTTP 服务器，默认监听 `8080` 端口。

**基础 URL**: `http://localhost:8080`

---

## 端点列表

### 1. 获取全局监控摘要

获取所有 Worker 的聚合统计数据。

**端点**: `GET /api/metrics/summary`

**响应示例**:
```json
{
  "timestamp": 1737123456,
  "total_workers": 3,
  "active_workers": 3,
  "total_tokens": 30,
  "available_tokens": 24,
  "cooldown_tokens": 6,
  "global_paint_rate": 12.5,
  "total_pixels_painted": 45678,
  "total_queue_size": 126,
  "avg_diff_count": 42
}
```

**字段说明**:
- `timestamp`: 当前时间戳（Unix 时间，秒）
- `total_workers`: Worker 总数
- `active_workers`: 活跃 Worker 数（最近上报过数据的）
- [total_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#226-230): 所有 Worker 的 Token 总数
- [available_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#231-254): 可用 Token 总数
- [cooldown_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#255-268): 冷却中 Token 总数
- `global_paint_rate`: 全局绘制速率（像素/秒）
- `total_pixels_painted`: 累计绘制总数
- `total_queue_size`: 队列总长度
- `avg_diff_count`: 平均差异像素数

---

### 2. 获取所有 Worker 列表

获取所有 Worker 的最新监控数据。

**端点**: `GET /api/metrics/workers`

**响应示例**:
```json
[
  {
    "worker_id": "luogu-logo-a3f2d5ab",
    "timestamp": 1737123456,
    "process_id": 12345,
    "start_time": 1737120000,
    "uptime_secs": 3456,
    "total_tokens": 10,
    "available_tokens": 8,
    "cooldown_tokens": 2,
    "total_pixels_painted": 15226,
    "pixels_painted_last_minute": 42,
    "paint_success_rate": 98.5,
    "avg_paint_delay_ms": 123.4,
    "queue_size": 42,
    "queue_peak_size": 128,
    "board_sync_diff_count": 15
  },
  {
    "worker_id": "banner-9f2e1d5b",
    "timestamp": 1737123456,
    ...
  }
]
```

**字段说明**:
- [worker_id](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/worker.rs#83-87): Worker 唯一标识（格式：`{图片名}-{位置哈希}`）
- `timestamp`: 数据采集时间戳
- `process_id`: Worker 进程 ID
- `start_time`: Worker 启动时间戳
- `uptime_secs`: 运行时长（秒）
- [total_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#226-230): Token 总数
- [available_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#231-254): 可用 Token 数
- [cooldown_tokens](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#255-268): 冷却中 Token 数
- `total_pixels_painted`: 累计绘制像素数
- `pixels_painted_last_minute`: 最近 1 分钟绘制像素数
- `paint_success_rate`: 绘制成功率（%）
- `avg_paint_delay_ms`: 平均绘制延迟（毫秒）
- `queue_size`: 当前像素队列长度
- `queue_peak_size`: 队列峰值长度
- `board_sync_diff_count`: 与目标图像的差异像素数

---

### 3. 获取特定 Worker 的最新数据

获取指定 Worker 的最新监控数据。

**端点**: `GET /api/metrics/worker/{worker_id}`

**路径参数**:
- [worker_id](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/worker.rs#83-87): Worker 唯一标识（例如：`luogu-logo-a3f2d5ab`）

**响应示例**:
```json
{
  "worker_id": "luogu-logo-a3f2d5ab",
  "timestamp": 1737123456,
  "process_id": 12345,
  "start_time": 1737120000,
  "uptime_secs": 3456,
  "total_tokens": 10,
  "available_tokens": 8,
  "cooldown_tokens": 2,
  "total_pixels_painted": 15226,
  "pixels_painted_last_minute": 42,
  "paint_success_rate": 98.5,
  "avg_paint_delay_ms": 123.4,
  "queue_size": 42,
  "queue_peak_size": 128,
  "board_sync_diff_count": 15
}
```

**错误响应**:
- `404 Not Found`: Worker 不存在或未上报数据

---

### 4. 获取 Worker 历史数据

获取指定 Worker 在一段时间内的历史监控数据。

**端点**: `GET /api/metrics/worker/{worker_id}/history`

**路径参数**:
- [worker_id](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/worker.rs#83-87): Worker 唯一标识

**查询参数**:
- [duration](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#190-199): 时间范围（可选，默认 `1h`）
  - 支持格式：`1h`（1小时）、`30m`（30分钟）、`1d`（1天）
  - 示例：`?duration=2h`

**响应示例**:
```json
[
  {
    "worker_id": "luogu-logo-a3f2d5ab",
    "timestamp": 1737123456,
    "total_pixels_painted": 15226,
    "paint_success_rate": 98.5,
    "avg_paint_delay_ms": 123.4,
    "queue_size": 42,
    ...
  },
  {
    "worker_id": "luogu-logo-a3f2d5ab",
    "timestamp": 1737123451,
    "total_pixels_painted": 15184,
    "paint_success_rate": 98.4,
    ...
  }
]
```

**说明**:
- 返回按时间倒序排列的历史数据点
- 每个数据点间隔约 5 秒（Worker 上报频率）
- 可用于绘制时间序列图表

---

### 5. 健康检查

检查监控系统是否正常运行。

**端点**: `GET /api/metrics/health`

**响应示例**:
```json
{
  "status": "ok",
  "master_uptime_secs": 12345,
  "active_workers": 3
}
```

**字段说明**:
- `status`: 服务状态（[ok](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/multi_token/token_manager.rs#210-225) 或 `error`）
- `master_uptime_secs`: Master 运行时长（秒）
- `active_workers`: 活跃 Worker 数量

---

## 启动配置

### Master 启动参数

```bash
./lgs-paintboard master \
  --ws-url wss://paintboard.luogu.me/api/paintboard/ws \
  --metrics-db ./metrics.db \
  --api-port 8080
```

**参数说明**:
- `--metrics-db`: SurrealDB 数据库路径（必需）
- `--api-port`: HTTP API 监听端口（可选，默认 8080）

### Worker 启动

Worker 无需特殊配置，只要连接到启用了监控的 Master，就会自动上报数据。

```bash
./lgs-paintboard worker \
  --master-socket /tmp/lgs_paintboard.sock \
  --config tokens.json \
  --image target.png
```

---

## 数据更新频率

- **Worker 上报频率**: 每 5 秒
- **分钟计数器重置**: 每 60 秒
- **API 响应**: 实时（从内存缓存读取）
- **数据库持久化**: 每次上报时写入

---

## 使用示例

### 使用 curl 查询

```bash
# 获取全局摘要
curl http://localhost:8080/api/metrics/summary

# 获取所有 Worker
curl http://localhost:8080/api/metrics/workers

# 获取特定 Worker
curl http://localhost:8080/api/metrics/worker/luogu-logo-a3f2d5ab

# 获取最近 2 小时的历史数据
curl "http://localhost:8080/api/metrics/worker/luogu-logo-a3f2d5ab/history?duration=2h"

# 健康检查
curl http://localhost:8080/api/metrics/health
```

### 使用 Python 查询

```python
import requests

# 获取全局摘要
response = requests.get('http://localhost:8080/api/metrics/summary')
summary = response.json()
print(f"全局绘制速率: {summary['global_paint_rate']} 像素/秒")

# 获取所有 Worker
workers = requests.get('http://localhost:8080/api/metrics/workers').json()
for worker in workers:
    print(f"{worker['worker_id']}: {worker['paint_success_rate']:.1f}% 成功率")
```

---

## 错误处理

### HTTP 状态码

- `200 OK`: 请求成功
- `404 Not Found`: Worker 不存在
- `500 Internal Server Error`: 服务器内部错误

### 错误响应格式

```json
{
  "error": "Worker not found: invalid-worker-id"
}
```

---

## 注意事项

1. **Worker ID 格式**: `{图片名}-{位置哈希}`
   - 示例：`luogu-logo-a3f2d5ab`
   - 图片名会自动清理特殊字符
   - 位置哈希基于起始坐标 (x, y)

2. **时间戳格式**: 所有时间戳均为 Unix 时间（秒）
   - 可使用 `new Date(timestamp * 1000)` 转换为 JavaScript Date

3. **数据延迟**: API 返回的是最近一次上报的数据（最多延迟 5 秒）

4. **历史数据保留**: 默认永久保留，可通过 [cleanup_old_data](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/metrics.rs#493-516) 方法清理

---

## 监控指标说明

### 绘制性能指标

- **绘制成功率**: 成功绘制的像素占总尝试的百分比
- **平均延迟**: 从发送请求到收到服务器响应的平均时间
- **绘制速率**: 最近 1 分钟绘制的像素数 ÷ 60 秒

### Token 状态指标

- **可用 Token**: 当前可以立即使用的 Token 数量
- **冷却中 Token**: 正在冷却期的 Token 数量
- **Token 利用率**: `available_tokens / total_tokens`

### 队列状态指标

- **队列长度**: 等待绘制的像素数量
- **队列峰值**: 历史最大队列长度
- **差异像素数**: 当前与目标图像不一致的像素数

---

## 数据持久化

监控数据存储在 SurrealDB（RocksDB 后端）中：

- **表名**: [worker_metrics](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/metrics.rs#403-412)
- **索引**: [worker_id](file:///home/xyber-nova/Github/LGS-Paintboard-Helper/src/app/ipc/worker.rs#83-87)、`timestamp`
- **Schema**: 强类型（SCHEMAFULL）
- **存储位置**: `--metrics-db` 指定的路径

可通过 SurrealDB 客户端直接查询历史数据：

```sql
SELECT * FROM worker_metrics
WHERE worker_id = 'luogu-logo-a3f2d5ab'
ORDER BY timestamp DESC
LIMIT 100;
```