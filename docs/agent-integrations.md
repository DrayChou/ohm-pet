# Agent integrations

[简体中文](agent-integrations.zh-CN.md) | English

OHM Pet accepts small local lifecycle signals from Claude Code, Codex, Pi Agent, and custom tools. The integration does not read terminal output or poll log files.

## Install from the tray

Open `Agent 集成` in the OHM Pet tray menu and select one or more entries:

- `接入 Claude Code`
- `接入 Codex`
- `接入 Pi Agent`
- `测试 Agent 动画`
- `移除全部 Agent 集成`

Installed entries show a checkmark. Clicking an installed entry updates its executable path and configuration. Installation and removal are idempotent.

## Event mapping

| External event | Pet state |
| --- | --- |
| Working or tool execution | Running |
| Waiting for input or permission | Waiting |
| Completed | Review for 4.2 seconds, then idle |
| Failed | Failed for 5.2 seconds, then idle |
| Session stopped | Idle |

OHM Pet aggregates status by agent source, session, and task. The activity bubble beside the pet shows up to five active tasks with titles, source labels when multiple agents are active, and elapsed time. Completed or failed tasks freeze their duration, remain briefly visible, and trigger an individual system notification.

Pi derives titles from the first line of the `before_agent_start` prompt. Claude Code metadata is extracted from hook stdin JSON. Codex thread, turn, and input titles are extracted when available in the `notify` JSON, while the public Codex notify interface still only guarantees completion events.

## Local signal command

Custom tools can use the same public command:

```bash
"/path/to/OHM Pet" signal --source custom --event working
"/path/to/OHM Pet" signal --source custom --event waiting
"/path/to/OHM Pet" signal --source custom --event completed
"/path/to/OHM Pet" signal --source custom --event failed
"/path/to/OHM Pet" signal --source custom --event idle

# Optional concurrent task identity and title
"/path/to/OHM Pet" signal --source custom --event working \
  --session-id session-1 --task-id task-1 --title "Prepare release notes"
```

The command sends one JSON datagram to `127.0.0.1:47832` and exits. No server is exposed outside localhost. If OHM Pet is not running, the signal is discarded.

## Claude Code

OHM Pet adds owned command hooks to `~/.claude/settings.json` while preserving unrelated settings and hooks. It subscribes to:

- `SessionStart`
- `UserPromptSubmit`
- `PreToolUse`
- `Notification` for permission, idle, and agent-input requests
- `Stop`
- `StopFailure`
- `SessionEnd`

Hooks use Claude Code's exec form with an absolute OHM Pet executable path. Run `/hooks` in Claude Code to inspect the installed entries.

## Codex

OHM Pet configures the top-level `notify` command in `${CODEX_HOME:-~/.codex}/config.toml`. Codex currently exposes only `agent-turn-complete` through this notification mechanism, so this integration reliably reports completion but cannot claim full start, tool, permission, or failure lifecycle coverage.

If a different Codex `notify` command already exists, OHM Pet stores it, forwards the original Codex JSON payload to it, and restores it when the integration is removed.

## Pi Agent

OHM Pet writes an extension to:

```text
~/.pi/agent/extensions/ohm-pet.ts
```

The extension subscribes to `agent_start`, `tool_execution_start`, `tool_execution_end`, `agent_settled`, and `session_shutdown`. It uses Node's built-in UDP module and starts no persistent helper process. Run `/reload` in an existing Pi session after installation, or restart Pi.

## Communication channel settings

Open `通讯渠道设置` from the pet context menu or system tray to configure Telegram Bot and Feishu/Lark application-bot channels. Settings are stored in `channels.json` inside the OHM Pet configuration directory; environment variables are no longer used. On macOS/Linux the file is restricted to the current user (`0600`). Saving produces a native status notification. Once a channel is complete, use `发送 Telegram 测试消息` or `发送飞书 / Lark 测试消息`; the request runs off the UI thread and reports success or a sanitized failure through a native notification.

Telegram requires a Bot Token, notification Chat ID, and a Telegram User ID allowlist for remote queries. Create the bot with `@BotFather`, send it one message, then inspect Bot API `getUpdates` for the `chat.id` and user ID.

If the computer cannot reach `api.telegram.org` directly, set an HTTP proxy under `通讯渠道设置 → 设置网络代理`, for example `http://127.0.0.1:7890`. OHM Pet does not depend on environment variables or guess the OS proxy. This setting is used for Telegram notifications, test messages, and long polling. A proxy URL containing credentials is stored in the permission-restricted `channels.json` and is never logged.

Feishu/Lark requires a custom-app App ID, App Secret, receive ID type (`open_id`, `chat_id`, `user_id`, or `email`), receive ID, and an `open_id` allowlist for inbound read-only commands.

Completed or failed tasks are sent to every enabled and complete channel. Messages use plain text and secrets are never logged.

Telegram now supports lightweight long-polling queries. A command reaches the desktop event loop only when both the configured Chat ID and User ID allowlist match:

```text
/status   aggregate status and first five tasks
/tasks    current task list
/help     command help
```

The update cursor is stored separately in `channel-state.json` so app restarts do not replay old commands; the file is not rewritten when no updates arrive.

> **Experimental / untested:** the Feishu/Lark implementation passes unit and compile checks, but real notifications, long connections, and replies have not completed end-to-end validation because valid custom-app credentials are not currently available. It should not be described as production-ready.

Feishu/Lark implements the same `/status`, `/tasks`, and `/help` commands through the official long-connection protocol. In the Feishu developer console, enable the bot, grant the required message permissions, subscribe to `im.message.receive_v1`, and select long connection as the event delivery method. Only `/` commands from an `allowedOpenIds` entry reach the desktop event loop, and responses are sent to the originating `chat_id`. Changing the App ID, App Secret, or enabled state rebuilds the connection automatically.

Remote commands are read-only and cannot execute shell commands, deploy, stop, or mutate tasks.

## Removal and safety

- Claude removal deletes only hook commands carrying the OHM Pet integration marker.
- Codex removal restores the previous `notify` command when one existed.
- Pi removal deletes the extension only when it contains the OHM Pet ownership marker.
- Malformed existing JSON or TOML is not overwritten; installation fails and the pet shows the failed animation.
- Local signals only select an animation state. They cannot run commands or access pet files.
