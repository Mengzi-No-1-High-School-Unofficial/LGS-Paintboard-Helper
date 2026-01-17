# LGS 冬日绘板助手 (LGS Paintboard Helper)

LGS 冬日绘板助手是一个功能强大的命令行工具，使用 Rust 编写，旨在帮助用户在洛谷冬日绘板上高效绘制图像。该工具支持多种绘制模式、多 Token 并发操作、本地画板同步、网格图算法优先级排序等高级功能。

我们用此项目维护了绘版上的蒙自一中校徽（200 * 200 大小），且在高强度随即撒点轰炸的情况下仍然能稳定维持图像主体！

<img width="2864" height="1548" alt="image" src="https://github.com/user-attachments/assets/8bc33a93-5f29-481a-ae90-1472e596f85b" />


## 🌟 功能特性

- **主从架构模式**: 支持 Master-Worker 架构,由 Master 负责全局同步,Worker 负责并发绘制
- **智能图像预处理**: 集成网格图算法,优先绘制重要区域以提高视觉效果
- **实时指标监控**: 提供详细的绘制统计信息
- **CD 时间管理**: 自动管理 Token 冷却时间,避免触发频率限制

## 📋 项目架构

本项目采用模块化设计，主要包括以下组件：

- `winter-paintboard-sdk`: 核心 SDK，提供与洛谷绘板 API 的交互功能
- `multi_token`: 多 Token 管理模块，负责并发绘制逻辑
- `board_sync`: 本地画板同步模块，维护本地画板状态
- `image_processing`: 图像处理模块，包含网格图算法等功能
- `export`: 数据导出模块，支持画板状态和热点图导出

## 🛠️ 技术栈

- **编程语言**: Rust
- **异步运行时**: Tokio
- **命令行解析**: Clap
- **图像处理**: Image, Imageproc
- **WebSocket 客户端**: 用于实时同步
- **日志系统**: Tracing, Log
- **序列化**: Serde, Serde_json

## 🚀 安装

### 前置要求

- Rust (1.70 或更高版本)
- Cargo

### 构建步骤

1. 克隆项目仓库：
   ```bash
   git clone https://github.com/Mengzi-No-1-High-School-Unofficial/LGS-Paintboard-Helper.git
   cd LGS-Paintboard-Helper
   ```

2. 构建项目：
   ```bash
   cargo build --release
   ```

3. 生成的可执行文件位于 `target/release/lgs-paintboard`

## 📖 使用说明

### 基础命令

```bash
# 查看帮助信息
./lgs-paintboard --help

### 主从绘制模式 (推荐)

本项目现在完全采用主从模式。

1. **启动 Master**: 负责同步画板数据
   ```bash
   ./lgs-paintboard master --ws-url <ws_url> --socket-path /tmp/paintboard.sock
   ```

2. **启动 Worker**: 执行绘制任务
   ```bash
   ./lgs-paintboard worker --master-socket /tmp/paintboard.sock --config tokens.json --image image.png --x 100 --y 100
   ```


## ⚙️ 配置文件

多 Token 模式需要配置文件来管理多个账户。配置文件格式如下：

```json
{
  "cd_time_ms": 3000,
 "tokens": [
    {
      "uid": 12345,
      "access_key": "your_access_key_here"
    },
    {
      "uid": 67890,
      "token": "your_pre_fetched_token_here"
    }
  ]
}
```

配置文件包含：
- `cd_time_ms`: Token 冷却时间（毫秒）
- `tokens`: Token 列表，每个 Token 可以使用 `access_key`（自动获取 Token）或直接使用 `token`

## 🎨 高级功能

### 像素惩罚机制

为避免多个 Token 重复绘制同一位置，工具实现了像素惩罚机制。该机制会降低近期被频繁绘制的像素的优先级。

- **时间衰减热力图**: 惩罚值基于像素在最近 **10 分钟**内的绘制频率（`R(i,j)`）计算，旧的绘制记录会自动过期。
- **归一化惩罚**: 惩罚值会根据近期总绘制数（`∑R`）进行归一化，避免在绘制初期产生过高的惩罚。
- **最终优先级计算**:
  - `惩罚值 = (R(i,j) / max(∑R, 最小总绘画数)) × 惩罚系数`
  - `最终优先级 = 基础优先级 - 惩罚值`

### 本地画板同步

Master 通过 WebSocket 实时同步画板状态,并通过 Unix Socket 分发给所有连接的 Worker,确保:
- 避免重复绘制已正确的像素
- 实时获取最新画板状态
- 极低的同步延迟


## 🛠️ 工具脚本

项目包含多个辅助工具脚本：

### get_token.py

用于批量获取绘板 Token 的 Python 脚本：
- 从 CSV 文件读取用户信息
- 自动获取 AccessKey 和 PaintKey
- 支持并发处理以提高效率

使用方法：
1. 准备 `tokens.csv` 文件，包含 UID 和保存站 Token
2. 运行 `python tools/get_token.py`
3. 脚本会自动填充 AccessKey 和 PaintKey 字段

### exports.py

用于处理导出的 Token 配置的 Python 脚本。

## 📊 指标监控

工具提供详细的绘制指标：
- 总绘制像素数
- 成功/失败绘制像素数
- 每个 Token 的绘制速率
- 实时绘制统计

指标每分钟更新一次，帮助用户监控绘制进度和性能。

## 🔒 安全与合规

- 所有认证信息仅在本地处理
- Token 通过安全的 API 获取
- 遵循洛谷保存站 API 的使用规范和频率限制
- AGPL-3.0 许可证，开源免费使用

## 🤝 贡献

欢迎提交 Issue 和 Pull Request 来改进项目。请确保：
- 遵循 Rust 代码规范
- 添加适当的测试
- 更新相关文档

## 📄 许可证

本项目采用 AGPL-3.0 许可证。详细信息请参见 [LICENSE](LICENSE) 文件。

## 🙏 致谢

感谢洛谷保存站（https://www.luogu.me）提供绘板平台，以及所有为本项目做出贡献的开发者。

---

> [!note]
> 
> 该程序中的一切人工智能生成代码，必须在未来经过完全的人工重构后使用！

> [!warning]
> 
> 请合理使用本工具，遵守洛谷平台的使用规范，避免对服务器造成过大压力。
