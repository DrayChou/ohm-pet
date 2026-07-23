# OHM Pet 通讯渠道开发计划

## 目标边界

通讯渠道只负责两类能力：

1. 把任务完成、失败和当前状态发送到用户已经配置的消息平台。
2. 把经过身份白名单和命令白名单校验的消息转换为内部 `ChannelCommand`。

渠道层不执行 Shell，不直接操作 Agent 会话，不把任意聊天文本解释成命令。

## 已完成（当前范围）

- `channels.json` 统一配置 Telegram 和飞书/Lark。
- macOS/Linux 配置文件权限为 `0600`，使用临时文件写入后替换。
- Telegram：完成/失败通知、long polling、Chat ID + User ID 白名单、更新游标防重放。
- 飞书：已实现完成/失败通知、官方 WebSocket 长连接、Protobuf 帧、分片、心跳、ACK、Open ID 白名单；当前标记为“实验性·未实测”，尚无真实凭据端到端验收。
- 两个渠道共用 `/status`、`/tasks`、`/help` 只读命令。
- 配置变更自动生效；网络工作不阻塞 UI；断线有固定退避。
- 远程回复限制长度，不记录 Token、Secret、鉴权 URL 或消息正文。
- 设置菜单支持两个渠道的“发送测试消息”，保存和测试结果通过系统通知显示。

## 发布前必须补齐

这些属于当前功能的完整性，不应推迟：

1. **真实渠道验收**：Telegram 已完成真实 Bot 通知、代理和命令验证；飞书仍需使用有效应用完成通知、命令、断线重连和配置变更测试。
2. **连接状态可诊断性**：测试消息已提供；发布前再确认 Telegram polling 冲突、飞书订阅缺失等接收端错误能以简短且不含敏感信息的形式呈现。
3. **配置损坏保护**：已实现拒绝覆盖；真实验收时确认用户通知包含可定位的配置路径。
4. **文档**：继续核对 Telegram ID 获取方式、飞书权限/事件订阅步骤、白名单为空时不接收命令。
5. **Release 性能验收**：测量未配置、仅 Telegram、仅飞书、两者同时启用四种状态的 CPU/RSS 和断网行为。

## 下一阶段（需要真实需求后再做）

### Agent 后继输入

只有确认 Pi/Claude Code 存在稳定、可定位具体会话的输入接口后，才增加：

```text
/continue <task-id> <message>
```

必须同时具备：

- task ID 与 Agent session ID 的稳定映射；
- `commandId`、过期时间和幂等去重；
- Agent 专用 Adapter，不通过 Shell 拼接命令；
- 明确的接受/失败回执；
- 会话已经结束时安全拒绝。

### 停止和审批

`/stop`、`/approve` 属于状态修改或高风险操作。必须先确认各 Agent 的官方控制接口和权限语义，再单独设计确认流程，不能复用自由文本通道直接执行。

## 明确不做

为避免过度开发，当前不做：

- 自建手机 App、PWA 或公网中继服务；
- 个人微信逆向协议；
- 通用脚本/Webhook 执行器；
- 任意自然语言自动映射为 Shell；
- 消息历史同步、全文搜索或聊天客户端功能；
- 多机器人、多 Telegram Chat、多飞书租户管理；
- 复杂工作流、审批引擎和 RBAC；
- 为只读命令引入数据库；
- 在没有真实需求前增加 Bark、企业微信、Matrix 等更多渠道。

## 后续实施顺序

1. 完成 Telegram 和飞书真实凭据验收，并检查接收端错误反馈。
2. 完成 Release 性能测试与打包验证。
3. 发布当前只读双向版本。
4. 收集是否确实需要移动端继续/停止任务的使用反馈。
5. 只有需求明确且 Agent 官方接口可用时，再实现单一 Agent 的受控后继输入试点。

## 验收命令

```text
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check --target x86_64-pc-windows-msvc -p ohm-pet
```
