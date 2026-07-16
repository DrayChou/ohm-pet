# 宠物包说明

简体中文 | [English](pet-packages.md)

## 默认素材源文件

仓库只包含一个默认宠物：

```text
assets/default-pets/ohm-raven/
├── pet.json
└── spritesheet.webp
```

打包脚本会把这个目录复制到面向用户的 `pets/` 目录。除非某个素材确定要公开发行，否则不要把私有或实验素材放进 `assets/default-pets/`。

## 用户宠物目录

Windows 便携包：

```text
OHM-Pet/
├── OHM Pet.exe
└── pets/
```

macOS 发行目录：

```text
发行目录/
├── OHM Pet.app
└── pets/
```

把宠物包放入 `pets/<pet-id>/`，然后在状态栏或系统托盘菜单中选择“刷新宠物目录”。

## Codex v2 格式

```text
pets/<pet-id>/
├── pet.json
└── spritesheet.webp
```

`pet.json` 必须包含：

```json
{
  "id": "example-pet",
  "displayName": "示例宠物",
  "description": "简短说明",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp"
}
```

v2 图集尺寸为 1536 × 2288，布局为 8 列 11 行，每格尺寸为 192 × 208。

## 自动发现

OHM Pet 还会读取以下目录中的 Codex 兼容宠物：

- `${CODEX_HOME:-~/.codex}/pets`
- `${CLAUDE_CONFIG_DIR:-~/.claude}/pets`
- 系统 Claude 应用支持目录下的 `pets/`

如果不同目录中出现相同 `id`，程序会按照目录优先级保留一个。用户明确指定的目录和应用旁边的本地目录优先于 Agent 管理目录。

Claude Code 当前没有官方宠物包规范。使用 Shimeji XML 和独立 PNG 帧的宠物包暂时不能直接由 Codex v2 加载器读取。
