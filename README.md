# 冬日绘板图片绘制工具

这是一个使用 Rust 编写的命令行工具，可以将 PNG 图片绘制到冬日绘板上。

> [!warning]
>
> 本程序中包含人工智能生成的代码，其中部分代码尚未经过完全的人工审查，请谨慎使用

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