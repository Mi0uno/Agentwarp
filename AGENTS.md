# Agent 项目约定

本项目克隆自用户 fork：`https://github.com/Mi0uno/Agentwarp.git`，用于后续二次开发。

## 开发流程硬性要求

- 每一个新功能点、修复点或较独立的改动开始前，必须先给出开发计划，并等待用户确认后再实施。
- 每一个功能点完成后，必须走完整测试流程并通过，才算真正完成。
- 完整测试流程应优先遵循项目已有 CI、脚本和文档约定；如果当前功能点没有更精确的项目流程，至少覆盖格式化检查、静态检查、相关单元测试、相关集成或端到端验证，以及必要的手工验证说明。
- 如果测试因为环境、依赖或外部服务问题无法完整执行，必须明确记录阻塞原因、已执行的验证项和剩余风险；不得把该功能点标记为完成。
- 所有后续开发改动都必须进行 Git 存档。每次开发都必须提交到 Git；每个功能点、修复点、文档或配置更新通过对应验证后，使用清晰提交信息单独提交，提交前确认 `git status` 中只暂存该变更相关文件。
- 不得把无关重构、格式化或用户未要求的改动混入功能提交。

## 分支、验收与合并要求

- 每个新的开发任务都必须从主分支创建独立 Git 分支，分支只承载该任务相关改动。
- 分支命名应简洁表达任务类型和范围，例如 `feature/<scope>`、`fix/<scope>` 或 `docs/<scope>`。
- 开发期间提交只进入当前任务分支；不得在用户验收前把任务分支合并回主分支。
- 需要编译验证的代码改动完成后，优先推送任务分支触发远端 Linux 开发构建，下载构建产物并正常安装运行可测试版本，让用户进行手工验收；文档类改动只需执行对应轻量验证。
- 提交给用户验收时，必须提供运行方式、已完成的自动化验证、远端 Linux 开发构建结果、已触发的跨平台 GitHub Actions 构建结果（如有）和需要重点测试的范围。
- 用户验收后必须主动询问“是否测试通过”。只有用户明确确认没有问题后，才允许把任务分支合并回主分支。
- 如果用户测试未通过或提出调整意见，必须继续在同一任务分支修复、重新编译运行、重新提交验收，直到用户确认通过。
- 合并回主分支后，必须确认主分支包含该任务的 Git 记录，并在交付汇报中说明任务分支名、主分支合并结果和最终提交号。

## 跨平台构建与缓存要求

- 开发与测试阶段优先使用专门的 Linux 开发构建 GitHub Actions workflow，目标是只构建 Linux `.deb` 并上传 artifact，避免本地机器长时间占用 CPU、内存和磁盘；如果仓库尚未提供该 workflow，应先补齐该 CI 配置或让用户确认临时方案。
- Linux 开发构建产物下载后，使用正常安装流程安装一次并以普通用户环境运行验证；不要用临时 `HOME` 验证 Claude Code agent，否则会丢失 `~/.claude` 认证与配置，导致误判为启动失败。
- 本地开发与功能点验证阶段，不得在本地手动触发 Windows 编译（包括安装 MSVC 工具链、跑 `cargo build --target x86_64-pc-windows-msvc` 等）；除非用户明确要求，也尽量避免本地完整 Linux 编译。
- Windows 编译由 GitHub Actions runners 统一负责，最终全量验证入口为 `.github/workflows/beta_release.yml`（tag `v*-beta*` 自动触发，或在 Actions 页面手动 `workflow_dispatch`）。
- 开发阶段不需要反复触发 Windows job；在远端 Linux 开发构建通过、下载产物安装运行通过、用户明确确认测试正常后，再新建 `v*-beta*` 标签或手动运行 `beta_release.yml` 做最终全套构建验证，等待 Linux package 和 `windows_installer`（x64 / arm64）job 全部成功后再继续合并流程。
- 编译时可以根据机器资源使用多线程并发，例如 `cargo build -j <N>` 或 `CARGO_BUILD_JOBS=<N>`；线程数以稳定通过构建、不过度占用内存和磁盘 I/O 为准。
- 每个功能点都必须同时考虑 Linux 和 Windows 适配，避免引入只在单一平台成立的路径、命令、依赖、权限、换行、终端、窗口系统或 shell 假设。
- 如果改动需要平台条件分支，必须分别说明 Linux 与 Windows 的行为；远端 Linux 开发构建 + 最终 GitHub Actions Windows 验证共同构成跨平台验证证据。
- 每次本地编译、构建验证或测试触发编译后，必须清理构建缓存和临时产物，避免 `target` 等目录持续占用磁盘空间；远端 Actions artifact 下载到本地后，也应在验收完成后清理下载包和临时解压目录。
- 清理缓存时优先使用项目或工具链提供的安全命令，例如 `cargo clean`；不得删除源码、用户改动、Git LFS 资源或其他未确认的工作文件。
- 交付汇报必须包含远端 Linux 开发构建结果、最终全量 GitHub Actions 构建结果（含 Windows x64 / arm64 job 链接或日志，如已触发）、测试结果、缓存清理结果和 Git 提交号。

## 推荐交付顺序

1. 明确需求和影响范围。
2. 制定开发计划并询问用户确认。
3. 从主分支创建独立任务分支。
4. 实施改动。
5. 运行与改动范围匹配的本地轻量验证，例如格式检查、静态检查或相关单元测试；避免不必要的本地完整编译。
6. 提交任务分支候选提交并推送，触发远端 Linux 开发构建。
7. 等待 Linux 开发构建成功，下载 `.deb` artifact，使用正常安装流程安装一次并运行可测试版本。
8. 交给用户手工验收，并主动询问用户是否测试通过。
9. 如果用户反馈失败或要求调整，继续在同一任务分支修复、提交、推送并重新走 Linux 开发构建与验收。
10. 用户确认测试通过后，按需新建 `v*-beta*` 标签或手动触发 `.github/workflows/beta_release.yml` 做最终全量构建验证；此阶段再等待 Windows x64 / arm64 job。
11. 最终全量构建通过后，清理本地下载产物、临时目录和本地产生的构建缓存。
12. 合并任务分支回主分支并推送。
13. 汇报任务分支、主分支合并结果、提交号、远端 Linux 开发构建结果、最终全量 GitHub Actions 构建结果、测试结果、缓存清理结果和剩余风险。

## 初始化会话记录

记录日期：2026-06-04。

- 项目目录：`/home/kali/Desktop/dev/Agentwarp`。
- 源仓库：用户 fork `https://github.com/Mi0uno/Agentwarp.git`。
- 首次项目约定文档提交：`330e9867 docs: add agent workflow requirements`。
- 已安装 Linux GUI 构建所需的系统依赖，包括 Git LFS、`pkg-config`、`cmake`、基础构建工具、ALSA/OpenSSL/Freetype/Fontconfig/libgit2/libclang 开发包、`clang-format`、`protobuf-compiler` 等。
- 已执行 `git lfs install --local` 和 `git lfs pull`，并确认 LFS 文件不再是 pointer 文本。
- GitHub 直接访问曾超时，后续网络相关命令使用过本机代理：`http://127.0.0.1:7897`。
- 运行 Linux GUI 的官方入口为：`WARP_SKIP_COMMON_SKILLS_INSTALL=1 ./script/run`。
- 2026-06-04 已通过官方入口完成一次 Linux GUI 编译和启动验证：编译通过，`target/debug/warp-oss` 成功启动，窗口标题为 `Warp`，窗口尺寸为 `1280x800`。
- 运行期观察到的非阻断提示：Wayland 剪贴板降级到 X11、EGL/DRI3 加速警告、SQLite WAL 恢复提示，以及上游当前存在的 Rust unused variable warnings。
- 验证结束后已关闭 GUI，并确认无 `warp-oss` 残留进程。
- 初始化验证后 `git status --short` 为空；`target` 构建目录约 `17G`。
- 不要在仓库文档、提交记录或脚本中记录 sudo 密码、token、SSH key 等敏感信息。

## Claude Code Agent 修复记录

记录日期：2026-06-16。

- 问题现象：Claude Code agent 切换模型或切换权限模式时会触发重启路径；如果当前 Claude 会话没有可恢复的 session id，旧逻辑仍按 resume 方式启动，导致切换失败，严重时表现为 Claude 无法正常启动。
- 修复范围：`app/src/workspace/view.rs` 中的 `cli_agent_restart_runtime_command_with_options`；当 Claude 没有可恢复 session id 时，回退到 `agent.command_with_runtime_options(...)` 发起全新启动，而不是强制 resume。
- 修复分支：`fix/claude-agent-restart-session-id`。
- 修复提交：`284b6dd0 fix: restart claude agent without resume id`。
- 主分支合并提交：`d8fae0725998b7e955733e95b1e602b2b4a723d6 merge: fix claude agent restart without resume id`。
- 验证标签：`v1.0.0-beta.claude-restart-20260616-074451`。
- GitHub Actions 全量 beta 构建已通过：`https://github.com/Mi0uno/Agentwarp/actions/runs/27615161854`。
- 已下载并通过 `pkexec dpkg -i` 正常安装 Linux `.deb` 包，安装版本为 `warp-terminal-oss 1.0.0-beta.claude-restart-20260616-074451`。
- 安装后按正常用户环境运行 Warp，并验证 Claude Code agent 启动、模型切换和权限模式切换；用户已确认“测试正常”。
- 后续验证 Claude Code agent 时必须使用正常安装后的用户环境，不要用临时 `HOME` 或临时配置目录复现，否则可能因为缺少 Claude 认证和配置产生误导性启动失败。
