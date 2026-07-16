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

## Local signal command

Custom tools can use the same public command:

```bash
"/path/to/OHM Pet" signal --source custom --event working
"/path/to/OHM Pet" signal --source custom --event waiting
"/path/to/OHM Pet" signal --source custom --event completed
"/path/to/OHM Pet" signal --source custom --event failed
"/path/to/OHM Pet" signal --source custom --event idle
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

## Removal and safety

- Claude removal deletes only hook commands carrying the OHM Pet integration marker.
- Codex removal restores the previous `notify` command when one existed.
- Pi removal deletes the extension only when it contains the OHM Pet ownership marker.
- Malformed existing JSON or TOML is not overwritten; installation fails and the pet shows the failed animation.
- Local signals only select an animation state. They cannot run commands or access pet files.
