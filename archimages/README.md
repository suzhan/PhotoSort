# ArchImages v3

照片归档 / 整理 / 重命名 / 查重桌面工具。Rust + Tauri 2 + Vue 3 重写，替代 Python 3.6 + PyQt5 的 ArchImages v2。

设计第一原则：**数据安全 > 性能**。扫描/规划零写入；源文件删除必须通过「复制完成 + 目标存在 + 大小一致 + 哈希一致」四前提；全程事务日志可恢复。

## 技术栈

- 前端：Vue 3 · TypeScript · Vite · Pinia · vue-i18n（zh-CN / en）
- 后端：Rust · Tauri 2 · Tokio · rusqlite · nom-exif（RAW 由 rawler 兜底）
- 终端用户零环境依赖：不需要 Python / ExifTool / Node / JVM / .NET

## 开发

```bash
npm install
npm run tauri dev      # 开发模式（启动桌面窗口）
```

## 质量门禁（每个 Phase 收尾必须全绿）

```bash
npm run typecheck
npm run build
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## 开发阶段

架构与 Phase 1–15 计划见 Phase 0 架构设计文档。当前进度：**Phase 1 项目初始化**。
