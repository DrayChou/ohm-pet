# 外部宠物素材格式

简体中文 | [English](external-pet-formats.md)

OHM Pet 只把导入包当作视觉素材处理。程序不会执行包内的 JAR、EXE、DLL、SAORI、SHIORI、ghost 或脚本文件。

## 安装方法

把宠物目录或 `.wlshm` 文件复制到面向用户的 `pets/` 目录，然后在状态栏或系统托盘中选择“刷新宠物目录”。格式检测会递归进行，因此可以直接复制完整的 Shimeji 或伪春菜目录，不需要手工打平文件结构。

## 支持的格式

### Codex v2

```text
pet-id/
├── pet.json
└── spritesheet.webp
```

这是 OHM Pet 原生运行格式。

### Shimeji-ee 及其兼容分支

支持以下常见结构：

```text
package/
├── conf/actions.xml
├── conf/behaviors.xml
└── img/<mascot>/shime*.png
```

也支持同一目录中包含 `actions.xml` 和 `shime*.png` 的素材。OHM Pet 会读取动作名称和 `Pose Image` 引用，把站立、行走、奔跑、跳跃、拖拽、下落、观察和挥手动作映射到 Codex/OHM 状态行。Java 代码和行为表达式不会执行。

### wl_shimeji

支持带 `WLPK` 文件头的 `.wlshm` 包。OHM Pet 会读取其中的 tar 数据、`manifest.json`、`actions.json` 和 QOI 图片，但忽略脚本和编译表达式。

### Ukagaka 和伪春菜 Shell

支持以下 Shell 结构：

```text
shell-name/
├── descript.txt
├── surfaces.txt
└── surface*.png
```

元数据支持 UTF-8 和 Shift-JIS。Surface 动画会归一化为待机、移动、跳跃、等待、失败和完成状态。这一适配覆盖 DesktopPet 中的茶兔素材和 MaidChan 使用的视觉 Shell。

### UkagakaW 独立视觉素材

如果目录是 `Resources/Bitmap/`，并包含多张 PNG，OHM Pet 会把它作为纯视觉备用宠物加载。这个规则用于没有标准 SSP Shell 的引擎仓库和原型素材。

### csaori 和其他插件仓库

csaori 是 SAORI/插件实现，不是宠物 Shell。OHM Pet 不会加载或执行其中的 DLL。如果某个包同时包含标准 `shell/` 目录，程序只加载视觉 Shell；只有插件而没有宠物图片的包不会错误生成宠物。

## 动作映射

| 原素材含义 | OHM 状态 |
| --- | --- |
| Stand、Sit、Idle、Surface 0 | 待机 |
| Walk、Run、Dash、移动 Surface 序列 | 向左或向右移动 |
| Wave、Greet、Look | 挥手或完成 |
| Jump、Bounce、Dragged、Pinched | 跳跃 |
| Trip、Fall、失败表情 Surface | 失败 |
| Sit、LookAtMouse、等待表情 | 等待 |

如果素材没有明确动作名称，程序会使用固定的 Surface 回退规则，保证宠物可以渲染。适配器不会假装完整复现原来的对话和行为引擎。

## 换装支持

当 Ukagaka 素材同时包含 `sakura.bindgroup<ID>.name` 元数据和对应的 `animation<ID>.pattern...,bind,...` PNG 叠加定义时，OHM Pet 会把它显示为可选服装。

换装入口：

- 状态栏或系统托盘中的“宠物换装”
- 右键单击宠物后弹出的上下文菜单

同一类别一次只能选择一个选项，因此可以同时选择一顶帽子和一套衣服。选择结果按宠物保存。缺少实际 PNG 的 bind 定义不会显示，避免出现点击后没有变化的假选项。

## 素材验证

不启动桌面窗口也可以验证整个素材目录：

```bash
cargo run -p ohm-pet-core --example validate_external -- /path/to/pets
```

验证器会解码所有宠物和所有已公布的服装。如果某个服装没有真正改变待机帧，验证会失败。

第三方真实素材应放在 Git 忽略的 `test-fixtures/external/` 目录。素材再分发权和源码许可证可能不同，不应直接提交到公开仓库。CI 使用 Rust 测试动态生成的合成素材。
