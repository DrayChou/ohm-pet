# OHM Pet

简体中文 | [English](README.md)

OHM Pet 是一个使用 Rust 开发的轻量桌面宠物，支持 macOS 和 Windows。它可以独立运行 Codex v2 宠物包，不需要持续打开 Codex，也不使用 WebView 或浏览器内核。

## 功能

- 透明、无边框的原生窗口
- 可切换的始终置顶模式
- 抓住宠物身体即可拖动，单击触发跳跃
- 鼠标进入身体最大尺寸 1.5 倍的范围后，宠物会看向鼠标
- 根据空闲时间、鼠标距离和近期互动选择低频自主动作
- 原生状态栏或系统托盘菜单
- 运行时切换宠物
- 自动发现本地、Codex 和 Claude 兼容宠物目录
- 一键接入 Claude Code、Codex 和 Pi Agent 生命周期事件
- 保存宠物选择、窗口位置和置顶设置
- 不运行 60 FPS 持续渲染循环

macOS 使用 AppKit 原生渲染。Windows 测试版使用 Win32 layered window 和每像素透明。正式 Windows GUI 构建使用 Windows 子系统，双击 `OHM Pet.exe` 时不会再弹出控制台窗口。

## 默认宠物和仓库目录

默认发行包只包含 OHM-1 欧姆鸦。仓库中的默认素材源文件位于：

```text
assets/default-pets/ohm-raven/
├── pet.json
└── spritesheet.webp
```

主要目录：

```text
assets/default-pets/   仓库内置的默认宠物素材
crates/ohm-pet-core/   宠物目录、图集、行为、状态机和配置
crates/ohm-pet-desktop/ macOS 和 Windows 原生桌面运行时
packaging/             macOS、Windows 图标和打包元数据
scripts/               图标生成和平台打包脚本
docs/                  架构与开发说明
dist/                  本机构建产物，不提交到 Git
```

构建后的 macOS 和 Windows 包会提供面向用户的 `pets/` 目录。把其他 Codex 兼容宠物放入该目录，然后在状态栏菜单中选择“刷新宠物目录”。完整格式与发现规则见[宠物包说明](docs/pet-packages.zh-CN.md)。

## 宠物自动发现

OHM Pet 会合并所有已发现目录中的兼容宠物。如果多个宠物使用相同的 `id`，优先使用排在前面的目录：

1. 环境变量 `OHM_PET_HOME`
2. 开发环境中的 `assets/default-pets/`
3. `OHM Pet.exe` 或 `OHM Pet.app` 旁边的 `pets/`
4. `${CODEX_HOME:-~/.codex}/pets`
5. `${CLAUDE_CONFIG_DIR:-~/.claude}/pets`
6. 系统 Claude 应用支持目录下的 `pets/`
7. macOS 应用包内部的备用宠物

Claude Code 当前没有官方内置的宠物包规范。OHM Pet 在 Claude 目录中查找的仍然是下方所述的 Codex v2 格式。使用 `actions.xml`、`behaviors.xml` 和独立 PNG 帧的 Shimeji 包暂未支持，后续需要单独增加格式适配器。

## Agent 集成

在状态栏或系统托盘中打开“Agent 集成”，即可安装或更新 Claude Code、Codex 和 Pi Agent 集成。OHM Pet 使用本地生命周期 Hook 和本机 UDP 事件，不抓取终端内容，也不轮询日志。

Claude Code 和 Pi 可以提供执行中、等待、完成、失败和空闲状态。Codex 官方 `notify` 机制目前只提供 `agent-turn-complete`，因此 Codex 集成负责完成通知，并会保留用户原来的 notify 命令。

安装位置、状态映射、自定义事件命令和移除规则见 [Agent 集成说明](docs/agent-integrations.zh-CN.md)。

## 宠物包格式

```text
pets/<pet-id>/
├── pet.json
└── spritesheet.webp
```

Sprite v2 图集固定为 8 列 11 行，总尺寸为 1536 × 2288，每格为 192 × 208。

## 开发

```bash
cargo run -p ohm-pet
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

指定自定义宠物目录：

```bash
OHM_PET_HOME=/path/to/pets cargo run -p ohm-pet
```

从欧姆鸦待机帧重新生成应用图标：

```bash
python3 scripts/generate-icons.py
```

## 构建 macOS 版本

```bash
./scripts/build-macos-app.sh
open "dist/OHM Pet.app"
```

脚本会生成临时签名的 macOS 应用，以及外部 `dist/pets/` 用户宠物目录。

## 构建 Windows 版本

在 Windows 中执行：

```powershell
./scripts/package-windows.ps1
```

便携压缩包输出到 `dist/windows/OHM-Pet-windows-x64.zip`。每次推送到 `main` 分支后，GitHub Actions 也会生成名为 `OHM-Pet-windows-x64` 的构建产物。

## 资源占用策略

- 原生事件循环会在任务间休眠。
- 待机动画每 480 毫秒切换一帧。
- 每 100 毫秒读取一次全局鼠标位置。
- 只有可见帧发生变化时才重绘。
- macOS 原生图像按需转换并缓存。
- 不驻留 Node.js、WebView 或浏览器进程。

## 许可证

MIT
