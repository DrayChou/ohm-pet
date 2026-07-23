# Agent 集成说明

简体中文 | [English](agent-integrations.md)

OHM Pet 可以接收 Claude Code、Codex、Pi Agent 和自定义工具发送的本地生命周期事件。整个过程不会读取终端输出，也不会轮询日志文件。

## 从状态栏菜单安装

打开 OHM Pet 状态栏或系统托盘中的“Agent 集成”，然后选择：

- “接入 Claude Code”
- “接入 Codex”
- “接入 Pi Agent”
- “测试 Agent 动画”
- “移除全部 Agent 集成”

已安装的项目会显示勾选标记。再次点击已安装项目，会更新可执行文件路径和配置。重复安装或移除不会产生重复配置。

## 状态映射

| 外部事件 | 宠物状态 |
| --- | --- |
| 开始工作或执行工具 | 执行中 |
| 等待输入或权限 | 等待 |
| 完成 | 显示完成动作 4.2 秒，然后恢复待机 |
| 失败 | 显示失败动作 5.2 秒，然后恢复待机 |
| 会话结束 | 待机 |

OHM Pet 会按 Agent 来源、会话和任务分别聚合状态。宠物旁的任务气泡最多显示 5 条活跃任务，并展示任务标题、来源（存在多个来源时）和持续时间。任务完成或失败后，耗时会冻结，条目短暂保留，并发送一条独立的系统通知。

Pi 会从 `before_agent_start` 的 prompt 首行生成任务标题；Claude Code 会从 Hook 标准输入 JSON 提取会话和 prompt；Codex 会从 `notify` JSON 中尽可能提取 thread、turn 和输入标题。Codex 的公开 `notify` 仍然只保证完成事件。

## 本地事件命令

其他工具也可以直接使用公开命令：

```bash
"/path/to/OHM Pet" signal --source custom --event working
"/path/to/OHM Pet" signal --source custom --event waiting
"/path/to/OHM Pet" signal --source custom --event completed
"/path/to/OHM Pet" signal --source custom --event failed
"/path/to/OHM Pet" signal --source custom --event idle

# 可选：区分并发任务并显示标题
"/path/to/OHM Pet" signal --source custom --event working \
  --session-id session-1 --task-id task-1 --title "整理发布说明"
```

命令会向 `127.0.0.1:47832` 发送一个 JSON 数据报，然后立即退出。服务不会监听外部网络。如果 OHM Pet 没有运行，该事件会被直接丢弃。

## Claude Code

OHM Pet 会在 `~/.claude/settings.json` 中增加带所有权标记的命令 Hook，并保留其他设置和已有 Hook。当前接入以下事件：

- `SessionStart`
- `UserPromptSubmit`
- `PreToolUse`
- `Notification` 中的权限、空闲和等待输入通知
- `Stop`
- `StopFailure`
- `SessionEnd`

Hook 使用 Claude Code 官方的 exec form，并保存 OHM Pet 可执行文件的绝对路径。安装后可以在 Claude Code 中运行 `/hooks` 查看。

## Codex

OHM Pet 会设置 `${CODEX_HOME:-~/.codex}/config.toml` 顶层的 `notify` 命令。Codex 当前只通过这个机制提供 `agent-turn-complete`，因此可以稳定接收完成事件，但不能声称已经覆盖开始、工具执行、等待权限和失败等完整生命周期。

如果用户原来已经设置了其他 `notify` 命令，OHM Pet 会保存它，把 Codex 原始 JSON 参数继续转发给它，并在移除集成时恢复原配置。

## Pi Agent

OHM Pet 会生成以下扩展：

```text
~/.pi/agent/extensions/ohm-pet.ts
```

扩展监听 `agent_start`、`tool_execution_start`、`tool_execution_end`、`agent_settled` 和 `session_shutdown`。它只使用 Node 内置 UDP 模块，不启动额外常驻进程。安装后，已打开的 Pi 会话需要执行 `/reload`，也可以直接重启 Pi。

## 通讯渠道设置

从宠物右键菜单或系统托盘打开“通讯渠道设置”，可以配置 Telegram Bot 和飞书/Lark 应用机器人。配置保存在 OHM Pet 配置目录下的 `channels.json`，不会再使用环境变量。macOS/Linux 会把文件权限限制为仅当前用户可读写（`0600`）。保存后菜单会显示系统通知；配置完整时还可以选择“发送 Telegram 测试消息”或“发送飞书 / Lark 测试消息”，发送过程在后台线程执行，成功或脱敏后的失败原因会通过系统通知显示。

Telegram 需要填写 Bot Token、接收通知的 Chat ID，以及允许远程查询的 Telegram User ID 白名单。Bot Token 可通过 `@BotFather` 创建机器人后获得；先向机器人发送一条消息，再调用 Bot API `getUpdates` 可查看 `chat.id` 和用户 ID。

如果本机无法直接访问 `api.telegram.org`，在“通讯渠道设置 → 设置网络代理”中填写 HTTP 代理，例如 `http://127.0.0.1:7890`。OHM Pet 不读取混乱的环境变量，也不会自动猜测系统代理；该配置用于 Telegram 通知、测试消息和 long polling。代理 URL 如包含账号密码，会和其他渠道凭据一样保存在权限受限的 `channels.json` 中且不会写入日志。

飞书/Lark 需要填写自建应用的 App ID、App Secret、接收 ID 类型（`open_id`、`chat_id`、`user_id` 或 `email`）、接收 ID，以及允许发送只读查询命令的 `open_id` 白名单。

任务完成或失败时，所有已启用且配置完整的渠道都会收到通知。消息使用纯文本，敏感 Token/Secret 不会写入日志。

Telegram 已支持轻量 long polling 双向查询。只有同时匹配配置的 Chat ID 和 User ID 白名单的命令才会进入桌面事件循环：

```text
/status   查看聚合状态和前 5 个任务
/tasks    查看当前任务列表
/help     查看命令帮助
```

更新游标保存在独立的 `channel-state.json` 中，避免应用重启后重复执行历史命令；没有新消息时不会写磁盘。

> **实验性 / 未实测：** 飞书/Lark 实现已经通过单元测试和编译检查，但由于尚未取得可用的飞书自建应用凭据，真实通知、长连接和消息回复目前未完成端到端验收，不应标记为生产可用。

飞书/Lark 已按官方长连接协议实现相同的 `/status`、`/tasks` 和 `/help`。需要在飞书开放平台为自建应用启用机器人、添加接收消息所需权限、订阅 `im.message.receive_v1`，并选择“使用长连接接收事件”。只有 `allowedOpenIds` 白名单内用户发送的 `/` 命令会进入桌面事件循环；回复会发送到原消息所在的 `chat_id`。App ID、App Secret 或启用状态发生变化时，连接会自动重建。

当前远程命令是只读的，不支持执行 Shell、部署、停止或修改任务。

## 移除和安全边界

- 移除 Claude 集成时，只删除带 OHM Pet 标记的 Hook。
- 移除 Codex 集成时，会恢复原来的 `notify` 命令。
- 只有 Pi 扩展包含 OHM Pet 所有权标记时，程序才会删除它。
- 如果已有 JSON 或 TOML 配置无法解析，程序不会覆盖文件。安装会失败，宠物显示失败动作。
- 本地事件只能切换宠物动画，不能执行命令，也不能读取宠物文件。
