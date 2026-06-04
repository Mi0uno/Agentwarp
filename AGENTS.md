# Agent 项目约定

本项目克隆自用户 fork：`https://github.com/Mi0uno/Agentwarp.git`，用于后续二次开发。

## 开发流程硬性要求

- 每一个新功能点、修复点或较独立的改动开始前，必须先给出开发计划，并等待用户确认后再实施。
- 每一个功能点完成后，必须走完整测试流程并通过，才算真正完成。
- 完整测试流程应优先遵循项目已有 CI、脚本和文档约定；如果当前功能点没有更精确的项目流程，至少覆盖格式化检查、静态检查、相关单元测试、相关集成或端到端验证，以及必要的手工验证说明。
- 如果测试因为环境、依赖或外部服务问题无法完整执行，必须明确记录阻塞原因、已执行的验证项和剩余风险；不得把该功能点标记为完成。
- 所有后续开发改动都必须进行 Git 存档。每个功能点通过完整测试后，使用清晰提交信息单独提交，提交前确认 `git status` 中只包含该功能点相关变更。
- 不得把无关重构、格式化或用户未要求的改动混入功能提交。

## 推荐交付顺序

1. 明确需求和影响范围。
2. 制定开发计划并询问用户确认。
3. 实施改动。
4. 运行完整测试流程。
5. 修复测试中发现的问题并重新验证。
6. 使用 Git 提交该功能点。
7. 汇报提交、测试结果和剩余风险。

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
