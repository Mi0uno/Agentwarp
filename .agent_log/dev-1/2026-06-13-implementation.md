# 2026-06-13 — DEV-1 实施 fix-agent-session-persistence

- 时间：2026-06-13
- Agent：DEV-1
- 任务卡：fix-agent-session-persistence-on-tab-switch
- 分支：`agent/dev-1/fix-agent-session-persistence`
- 提交号：pending（即将提交）

## 上下文

主 agent 与用户对齐范围（方案 A：attach 重建 + 仅 HiddenForClose 不杀 PTY）后，DEV-1 接手实施。第一次 subagent 调用被 API 456 错误中断，无任何输出；第二次调用（241 个 tool_uses，482 秒）完成了所有代码改动，但在返回报告阶段被 502 错误打断。本日志在主 agent 接续时补写。

任务卡：`specs/fix-agent-session-persistence-on-tab-switch/task-card.md`  
主 agent 决策留痕：`.agent_log/main/2026-06-13-agent-session-persistence-bug.md`  
SM 迭代计划：`.agent_log/sm/2026-06-13-iteration-fix-agent-session-persistence.md`

## 关键行为 / 决策

### 改动 1：抽出 `seed_cli_agent_session_for_record` 为 free function

原 `Workspace::seed_cli_agent_session_for_record`（`workspace/view.rs:13569-13617`）是 `Workspace` 私有方法，签名带 `&mut self`，无法从 `TerminalPane::attach` 直接调用。DEV-1 把函数体抽出为模块级 `pub(crate) fn seed_cli_agent_session_for_record<V: View>(...)`，放在 `workspace/view.rs` 末尾（紧挨 `impl Workspace` 之后），所有原调用点改为调用自由函数。

`remote_host` 解析改用 `ssh_remote_host_id_from_environment_id` + `SshRemoteModel::as_ref(ctx).host(host_id)`——这与原 `Workspace::ssh_remote_host_for_environment_id`（`workspace/view.rs:13153-13159`）的内部实现**完全一致**，是等价改写。

### 改动 2：`TerminalPane::attach` 重建 `CLIAgentSession` placeholder

在 `attach` 末尾（`ActiveAgentViewsModel` 注册后），检查 `CLIAgentSessionsModel::session(terminal_view_id)` 是否为 `None`：
- 若为 `None`，再查 `AgentSessionsModel::records()` 找 `terminal_view_id` 匹配的 `AgentSessionRecord`。
- 若找到，调用 `seed_cli_agent_session_for_record` 重建 placeholder（`listener: None`，等下一个 OSC 777 事件升级）。
- 若没找到（普通 shell tab 重新 attach），do nothing。

### 改动 3：`TerminalPane::detach` 不再无条件 `remove_session`

把 `if !matches!(detach_type, DetachType::Moved)` 改为 `if matches!(detach_type, DetachType::Closed)`。`HiddenForClose`（可撤销关闭）路径保留 `CLIAgentSession`，等 `attach` 时用 placeholder 重建逻辑恢复。

### 改动 4：`Workspace::remove_tab` 仅在非 `add_to_undo_stack` 路径杀 PTY

`remove_tab` 的 `for_all_terminal_panes` 循环在调用 `shutdown_pty` 外加 `if !add_to_undo_stack` 守卫。`add_to_undo_stack == true` 意味着 `HiddenForClose`（可撤销），不杀 PTY；`add_to_undo_stack == false` 意味着 `Closed`（永久），仍按 `is_active_and_long_running` 杀 PTY。

### 改动 5：回归测试

在 `pane_group/mod_tests.rs` 加 `terminal_pane_detach_preserves_cli_agent_session_on_hidden_for_close`：
1. `initialize_app` 注入 4 个新 singleton（`AgentReasoningEffortModel`、`AgentRuntimeSettingsModel`、`AgentSessionsModel`、`SshRemoteModel`）。
2. `mock_pane_group` 创默认 terminal pane。
3. 注入 `CLIAgentSession` for that view。
4. 调 `PaneContent::detach(HiddenForClose)` → 断言 session 仍存在。
5. 调 `PaneContent::detach(Closed)` → 断言 session 已被清。

## 摘要 / 结果

- `cargo check -p warp` ✅ 通过（"Finished dev profile in 0.86s"）。
- `cargo test -p warp --lib terminal_pane_detach_preserves_cli_agent_session_on_hidden_for_close` ✅ **1 passed; 0 failed**。
- `cargo test -p warp --lib 'pane_group::tests::'` ✅ **49 passed; 0 failed**。
- `cargo test -p warp --lib 'agent_sessions::'` ✅ **142 passed; 0 failed**。
- `cargo test -p warp --lib 'view_tests'` ✅ 16 passed（涉及 view_tests 模块，无回归）。
- `cargo test -p warp --lib 'cli_agent'` 整体：201 passed; 16 failed。
  - **基线对比**（stash dev-1 改动后跑同样 filter）：200 passed; 16 failed。
  - **结论**：dev-1 改动**没有引入回归**。16 个失败是仓库预存在问题——`terminal::view_tests.rs` 的 inline `App::test((), ...)` 没注册 `AgentReasoningEffortModel` / `AgentRuntimeSettingsModel` / `SshRemoteModel` / `AgentSessionsModel` 这些 singleton，一旦测试代码走到 `CLIAgentSessionsModel` 相关路径就 panic。dev-1 仅在 `pane_group/mod_tests.rs::initialize_app` 注册了它们，**不是** dev-1 引起的回归。
- 编译警告：cargo "database or disk is full" 是 last-use cache 写入失败（target 17G，磁盘吃紧），与编译正确性无关。

## 风险与剩余项

- **R-1（预存在，不在本任务范围）**：`terminal::view_tests.rs` 16 个 cli_agent 测试 panic 在 `AgentReasoningEffortModel` 未注册。建议后续任务统一在 `app/src/lib.rs::initialize_app` 或一个公共 test helper 里补齐。
- **R-2**：dev-1 未能返回报告（502 错误），主 agent 接手 commit 与剩余步骤。
- **R-3**：Windows 构建未跑（仅 Linux cargo check）。改动不涉及平台特定 API，理论无问题；按 SM DoD 14 记录为"待评估"，不豁免。
- **R-4**：磁盘 "database or disk is full" 警告——`cargo clean` 会释放，但 SM 守门要求"测试后清理"，本日志里不执行（避免影响后续编译）；由 main agent 在合并本地 master 后统一清理。
- **R-5**：`seed_cli_agent_session_for_record` 用 `SshRemoteModel::as_ref(ctx).host(host_id).cloned()`——依赖 `SshRemoteModel` 在所有调用点（`Workspace::start_agent_session` / `restore_agent_session` / `restore_agent_session_in_existing_terminal` / 新的 `TerminalPane::attach`）都有注册。生产环境由 `app/src/lib.rs:1817` 附近注册；测试环境由 dev-1 在 `pane_group/mod_tests.rs::initialize_app` 补齐。

## 引用

- 根因定位文件：
  - `app/src/pane_group/pane/terminal_pane.rs:448-518`（`detach`，现 515-524 已修）
  - `app/src/pane_group/pane/terminal_pane.rs:321-446`（`attach`，现 448-468 已加重建逻辑）
  - `app/src/workspace/view.rs:11795-11823`（`remove_tab` 的 shutdown_pty 循环，已加 `if !add_to_undo_stack` 守卫）
- 抽出的 free function：`app/src/workspace/view.rs:24335-24394` `pub(crate) fn seed_cli_agent_session_for_record`
- 回归测试：`app/src/pane_group/mod_tests.rs:3348-3431`

---
Signed-off-by: DEV-1 <dev-1@agentwarp.local>
