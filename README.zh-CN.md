# PhotoSort

[English](README.md) | [中文](README.zh-CN.md)

一款由元数据驱动的跨平台照片整理工具。

**设计原则：数据安全优先，性能第二。** 扫描和预览计划不会写入磁盘。只有在复制完成、目标文件存在、文件大小一致、哈希一致这四项校验全部通过后，才会删除源文件。完整的任务日志用于支持崩溃恢复。

![PhotoSort 主界面](docs/screenshot-main.png)

*主界面 - 选择来源/目标目录，配置整理规则，并在执行前预览结果。*

![PhotoSort 设置](docs/screenshot-settings.png)

*高级设置 - 重复检测模式、并发数、EXIF fallback、Google Maps 逆地理编码以及安全的 API Key 存储。*

## 功能

- **递归扫描**，使用经过验证的扩展名白名单，覆盖 `nom-exif` 与 `rawler` 支持的格式
- **EXIF 元数据读取**，标准图片使用 `nom-exif`，相机 RAW 使用 `rawler`，并可选使用 ExifTool 作为运行时 fallback
- **模板引擎**，用于定义目录和文件名规则，例如 `{yyyy}/{camera_model}/{gps_city}`、`{yyyyMMdd}_{HHmmss}_{seq:4}`
- **只读预览**，真正整理前先生成计划，预览和执行使用同一套处理流程
- **重复检测**，基于 SHA-256 内容哈希
- **安全文件操作**，原子复制、跨盘移动 fallback、复制校验后再删除源文件
- **后台任务**，支持受控并发、进度事件和取消
- **SQLite 持久化**，保存哈希缓存、GPS 缓存和任务日志，并支持崩溃恢复
- **Google Maps 逆地理编码**，可选启用，API Key 存储在系统原生凭据库中
- **跨平台路径安全**，处理 Windows 保留名、非法字符、路径穿越和长度限制
- **国际化界面**，支持英文和简体中文

## 模板系统

PhotoSort 使用灵活的模板引擎来定义照片如何归档到目录以及如何重命名。模板使用 `{variable}` 语法，并由专门的 tokenizer 解析，避免简单字符串替换带来的歧义。

### 可用变量

#### 日期与时间

| 变量 | 说明 | 示例 |
|---|---|---|
| `{yyyy}` | 四位年份 | `2017` |
| `{MM}` | 两位月份 | `11` |
| `{dd}` | 两位日期 | `30` |
| `{yyyyMMdd}` | 完整日期 | `20171130` |
| `{yyyy-MM-dd}` | 带横线日期 | `2017-11-30` |
| `{HH}` | 小时，24 小时制 | `15` |
| `{mm}` | 分钟 | `22` |
| `{ss}` | 秒 | `31` |
| `{HHmmss}` | 完整时间 | `152231` |

#### 相机与镜头

| 变量 | 说明 | 示例 |
|---|---|---|
| `{camera_make}` | 相机制造商 | `NIKON CORPORATION` |
| `{camera_model}` | 相机型号 | `NIKON D80` |
| `{lens_make}` | 镜头制造商 | `NIKON` |
| `{lens_model}` | 镜头型号 | `18-135mm F3.5-5.6` |

#### GPS / 位置

| 变量 | 说明 | 示例 |
|---|---|---|
| `{gps_country}` | 国家或地区 | `China` |
| `{gps_province}` | 省 / 州 | `Guangdong` |
| `{gps_city}` | 城市 | `Hong Kong` |
| `{gps_district}` | 区域 | `Central` |

#### 文件

| 变量 | 说明 | 示例 |
|---|---|---|
| `{original_name}` | 原始文件名，不含扩展名 | `DSC_1231` |
| `{extension}` | 文件扩展名 | `JPG` |
| `{seq}` | 自动递增序号 | `1` |
| `{seq:4}` | 补零序号，宽度 1-10 | `0001` |

### 目录模板示例

目录模板定义目标根目录下的文件夹结构。使用 `/` 分隔层级。

```text
{yyyy}/{camera_model}
-> 2017/NIKON D80/

{yyyy}/{gps_city}/{camera_model}
-> 2017/Hong Kong/NIKON D80/

{yyyy}/{yyyyMMdd}/{camera_model}
-> 2017/20171130/NIKON D80/

{yyyy}/{yyyy-MM-dd}/{camera_model}/{lens_model}
-> 2017/2017-11-30/NIKON D80/18-135mm F3.5-5.6/
```

### 文件名模板示例

文件名模板定义最终文件名，不包含目录路径。

```text
{original_name}.{extension}
-> DSC_1231.JPG

{yyyyMMdd}_{HHmmss}.{extension}
-> 20171130_152231.JPG

{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}
-> 20171130_152231_0001.JPG

{original_name}_{yyyyMMdd}.{extension}
-> DSC_1231_20171130.JPG

{seq:5}.{extension}
-> 00001.JPG
```

### 规则

- `{seq}` **只能用于文件名模板**，不能用于目录模板，因为目录中的序号无法保证确定性。
- 序号按目标目录分别计数，并由并发安全的协调器分配，确保并行处理时也不会冲突。
- 使用 `{{` 和 `}}` 输出字面量花括号，例如 `{{literal}}` -> `{literal}`。
- 缺失元数据会自动使用可配置的 fallback 名称，默认值为 `UnknownCamera`、`UnknownLocation`、`UnknownDate`，可在设置中修改。
- 所有路径片段都会做跨平台安全处理，包括 Windows 保留名、非法字符、末尾点/空格和 `../` 路径穿越。

## 技术栈

| 层 | 技术 |
|---|---|
| 前端 | Vue 3, TypeScript, Vite, Pinia, vue-i18n |
| 后端 | Rust, Tauri 2, Tokio, rusqlite, nom-exif, rawler |
| 哈希 | sha2, sha1, md-5 |
| HTTP | reqwest，使用 rustls-tls，无 OpenSSL 依赖 |
| 密钥 | keyring，系统原生 Keychain / Credential Manager |

最终用户安装后不需要额外运行时依赖，不需要 Python、ExifTool、Node、JVM 或 .NET。

## 项目结构

```text
archimages/
├── src/                      # Vue 3 前端
│   ├── components/           # DirectoryPicker, RuleEditor, ScanResultTable, ...
│   ├── views/                # MainView
│   ├── stores/               # Pinia: settings, scan, task, log
│   ├── types/                # 与 Rust 模型对应的 TypeScript DTO
│   ├── services/             # tauri.ts - 集中管理 IPC
│   └── i18n/                 # en, zh-CN
└── src-tauri/
    └── src/
        ├── commands/         # Tauri IPC: scan, organize, settings, jobs, geocode
        ├── core/             # scanner, metadata, template, planner, hash,
        │                     # duplicate, file_ops, organizer, task_manager,
        │                     # geocode, api_key
        ├── db/               # schema, hash_cache, gps_cache, jobs
        ├── models/           # PhotoFile, PhotoMetadata, PhotoPlan, ...
        ├── config/           # JsonSettingsStore
        ├── error/            # AppError
        └── utils/            # path safety, logging
```

## 开发

### 前置条件

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/) stable toolchain
- Tauri 平台依赖：
  - **macOS**: Xcode Command Line Tools
  - **Linux**: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf`
  - **Windows**: MSVC build tools

### 运行

```bash
cd archimages
npm install
npm run tauri dev
```

### 质量检查

每次提交前建议运行：

```bash
# Frontend
npm run typecheck
npm run test
npm run build

# Backend
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### codec-corpus 集成测试

集成测试使用 [imazen/codec-corpus](https://github.com/imazen/codec-corpus) 中的真实图片。测试数据不会提交到 git：

```bash
./tests/download_codec_corpus.sh
cd archimages/src-tauri && cargo test --test codec_corpus
```

## 构建与发布

```bash
cd archimages
npm run tauri build
```

生成平台原生安装包：
- **Windows**: NSIS 安装器 (`.exe`)
- **macOS**: `.app` 应用包和 `.dmg` 磁盘镜像，支持 Apple Silicon 与 Intel

当前 release 是 **unsigned beta** 构建。代码签名、Apple Developer ID 和 notarization 会在后续稳定版本中配置。

在 macOS 上，从浏览器下载的未签名构建可能会提示“已损坏”或“无法打开”。如果你确认下载的 release 资产可信，可以先移除 quarantine 属性再启动：

```bash
xattr -dr com.apple.quarantine /Applications/PhotoSort.app
```

## CI

GitHub Actions (`.github/workflows/ci.yml`) 会在每次 push 和 pull request 时运行：
1. **质量检查** (Ubuntu) - fmt, clippy, test, typecheck, frontend build
2. **构建 Windows** (x64) - NSIS `.exe` 产物
3. **构建 macOS** (Apple Silicon + Intel) - `.app` + `.dmg` 产物

## 安全说明

- Google Maps API Key 存储在系统原生凭据库中，不会写入 `settings.json` 或源码。
- 后端会重新校验前端传入的所有路径，确保生成目标不会逃逸出目标根目录。
- 哈希校验失败时永远不会删除源文件；不确定时会保留两份文件。

## 许可证

[MIT License](LICENSE)
