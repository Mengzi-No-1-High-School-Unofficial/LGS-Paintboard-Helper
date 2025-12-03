# LGS 冬日绘板助手 (LGS Paintboard Helper)

LGS 冬日绘板助手是一个功能强大的命令行工具，使用 Rust 编写，旨在帮助用户在洛谷冬日绘板上高效绘制图像。该工具支持多种绘制模式、多 Token 并发操作、本地画板同步、网格图算法优先级排序等高级功能。

我们用此项目维护了绘版上的蒙自一中校徽（200 * 200 大小），且在高强度随即撒点轰炸的情况下仍然能稳定维持图像主体！

<img width="2864" height="1548" alt="image" src="https://github.com/user-attachments/assets/8bc33a93-5f29-481a-ae90-1472e596f85b" />


## 🌟 功能特性

- **多 Token 并发绘制**: 支持使用多个用户 Token 同时绘制，大幅提升绘制效率
- **智能图像预处理**: 集成网格图算法，优先绘制重要区域以提高视觉效果
- **本地画板同步**: 实时同步本地画板状态，避免重复绘制和冲突
- **灵活绘制模式**: 支持增量修改、循环绘制和单次绘制等多种模式
- **实时指标监控**: 提供详细的绘制统计信息
- **自动导出功能**: 支持定期导出当前画板状态和热点图
- **CD 时间管理**: 自动管理 Token 冷却时间，避免触发频率限制

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

# 查看多 Token 模式帮助
./lgs-paintboard multi-token --help

# 查看项目信息
./lgs-paintboard about

# 获取当前画板状态
./lgs-paintboard get-board --uid <uid> --access-key <access_key>
```

### 多 Token 绘制模式

多 Token 模式是本工具的核心功能，允许使用多个账户同时绘制图像：

```bash
./lgs-paintboard multi-token --config config.json --image image.png --x 0 --y 0
```

#### 参数说明

- `--config`: Token 配置文件路径（JSON 格式）
- `--image`: 要绘制的图片路径（支持 PNG、JPG 等格式）
- `--x`, `--y`: 绘制起始坐标
- `--width`, `--height`: 图片缩放尺寸（可选）
- `--cd-time`: Token 冷却时间（毫秒，默认 3000）
- `--comparison-interval`: 画板比对间隔（毫秒，默认 5000）
- `--enable-export`: 启用画板状态导出
- `--enable-heatmap-export`: 启用热点图导出
- `--export-dir`: 导出目录（默认 "exports"）
- `--export-interval`: 导出间隔（秒，默认 30）
- `--canny-low-thresh`, `--canny-high-thresh`: 网格图算法阈值
- `--penalty-scale`: 惩罚系数（用于避免重复绘制同一位置）

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

为避免多个 Token 重复绘制同一位置，工具实现了像素惩罚机制：
- 根据 10 分钟内的绘制频率计算惩罚值
- 频繁被绘制的像素优先级会降低
- 惩罚公式：`P(i,j) = C(i,j) - (R(i,j) / max(∑R, 最小总绘画数)) × 惩罚系数`

### 本地画板同步

工具通过 WebSocket 实时同步画板状态到本地，确保：
- 避免重复绘制已正确的像素
- 实时获取最新画板状态
- 提高绘制效率


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
