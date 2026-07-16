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

## 本地事件命令

其他工具也可以直接使用公开命令：

```bash
"/path/to/OHM Pet" signal --source custom --event working
"/path/to/OHM Pet" signal --source custom --event waiting
"/path/to/OHM Pet" signal --source custom --event completed
"/path/to/OHM Pet" signal --source custom --event failed
"/path/to/OHM Pet" signal --source custom --event idle
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

## 移除和安全边界

- 移除 Claude 集成时，只删除带 OHM Pet 标记的 Hook。
- 移除 Codex 集成时，会恢复原来的 `notify` 命令。
- 只有 Pi 扩展包含 OHM Pet 所有权标记时，程序才会删除它。
- 如果已有 JSON 或 TOML 配置无法解析，程序不会覆盖文件。安装会失败，宠物显示失败动作。
- 本地事件只能切换宠物动画，不能执行命令，也不能读取宠物文件。
