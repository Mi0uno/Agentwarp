# Agent 项目约定

本项目克隆自用户 fork：`https://github.com/Mi0uno/Agentwarp.git`，用于后续二次开发。

## 开发流程硬性要求

- 每一个新功能点、修复点或较独立的改动开始前，必须先给出开发计划，并等待用户确认后再实施。
- 每一个功能点完成后，必须走完整测试流程并通过，才算真正完成。
- 完整测试流程应优先遵循项目已有 CI、脚本和文档约定；如果当前功能点没有更精确的项目流程，至少覆盖格式化检查、静态检查、相关单元测试、相关集成或端到端验证，以及必要的手工验证说明。
- 所有本地 `cargo` / `rustc` 步骤都必须遵守下方"本地编译全面交给 GitHub Actions"小节；本机（16C/15G/88-93% 磁盘）默认**不允许**任何本地编译。
- 如果测试因为环境、依赖或外部服务问题无法完整执行，必须明确记录阻塞原因、已执行的验证项和剩余风险；不得把该功能点标记为完成。
- 所有后续开发改动都必须进行 Git 存档。每次开发都必须提交到 Git；每个功能点、修复点、文档或配置更新通过对应验证后，使用清晰提交信息单独提交，提交前确认 `git status` 中只暂存该变更相关文件。
- 不得把无关重构、格式化或用户未要求的改动混入功能提交。

## 分支、验收与合并要求

- 项目只维护两类分支，**任何人、任何自动化工具都不能打破这个区分**：
  - **主分支** `master`：只接受已完成、已通过 Actions 编译+测试+手工验收的任务分支合并；**任何人都不能直接 push 到 `master`**，改 `master` 的唯一途径是通过任务分支提 PR 并 merge。
  - **任务分支** `feature/<scope>` / `fix/<scope>` / `docs/<scope>` / `chore/<scope>`：每个功能点、修复点、文档或配置改动从最新 `master` 切出，**只承载该任务相关改动**，开发期间所有 commit 都进任务分支。
  - `<scope>` 必须是 kebab-case、简洁（≤ 4 个单词），并能直接读出任务范围。
  - 任务分支被搞乱时，先尝试 `git cherry-pick` 或把改动搬到新分支；**绝对不能**把没合并的改动随手丢掉。
- 开发期间提交只进入当前任务分支；不得在用户验收前把任务分支合并回主分支。
- 每次开发完成后，必须在任务分支上 push，触发 `.github/workflows/dev_build.yml` 编译出 `warp-oss-linux-x64` artifact，本地用 `gh run download` 拉下来运行做手工验收；其他需要 Windows 验证的改动额外跑 `beta_release.yml`。
- 提交给用户验收时，必须提供运行方式、artifact 下载命令、已完成的自动化验证、Linux/Windows 构建结果和需要重点测试的范围。
- 用户验收后必须主动询问“是否测试通过”。只有用户明确确认没有问题后，才允许把任务分支合并回主分支。
- 如果用户测试未通过或提出调整意见，必须继续在同一任务分支修复、重新 push 触发 `dev_build.yml`、重新提交验收，直到用户确认通过。
- 合并回主分支后，必须确认主分支包含该任务的 Git 记录，并在交付汇报中说明任务分支名、主分支合并结果、最终 commit SHA 和对应 artifact 的 `BUILD_SHA` 是否一致。

## 跨平台构建与缓存要求

- **本地不构建**：本机不再跑 `cargo build` / `cargo run` / `cargo test --no-run` / `cargo install`，详见下方"本地编译全面交给 GitHub Actions"小节。所有编译产物一律来自 GitHub Actions。
- **Linux GUI 编译入口**：任务分支 push 后由 `.github/workflows/dev_build.yml` 自动跑（`ubuntu-latest-large`，16C/64G），产出 `warp-oss-linux-x64` artifact，本地 `gh run download` 拉取。
- **Windows 编译入口**：`.github/workflows/beta_release.yml` 的 `windows_installer` job（x64 + arm64 矩阵）。**每个涉及平台相关改动的功能点合并回 `master` 之前**必须至少触发一次 Windows 编译验证：push 一个临时 `v*-beta*` 标签或在 Actions 页面用 `workflow_dispatch` 手动运行 `beta_release.yml`，等待两个 `windows_installer` job 全部成功，再继续合并流程；不得仅凭 Linux artifact 合并涉及平台相关改动的功能点。
- **CI 编译入口**：`.github/workflows/ci.yml`（PR 触发）和 `.github/workflows/populate_build_cache.yml`（master 缓存预热），跟本地开发无关，由 CI 自动跑。
- **编译参数**：本地不适用；CI / Actions 端由 `ubuntu-latest-large` / `windows-latest-large` 默认并行 16 核，**不要**在 workflow 里手动加 `-j` 限制。

## 本地编译全面交给 GitHub Actions（硬性要求）

本机是 16 核 / 15 GiB 内存 / 79G 根分区（已用 88-93%）的笔记本；Agentwarp 一次 `cargo build --release` 增量即吃 8-12 GiB RSS、`target/` 稳定 17-28G；之前一次 `cargo clean` 释放了 ~24.8 GiB。**默认 cargo 并行（nproc）会把机器卡到无响应、触发 OOM 或磁盘写满**，且全量重编一次要十几到几十分钟，迭代效率极低。

因此，从本次会话起调整编译策略：

- **本地不再 `cargo build` / `cargo run` / `cargo test --no-run`** 任何需要实际链接产物的命令。所有编译/链接工作统一交给 GitHub Actions 大机器。
- **本地仍允许的轻量命令**：仅限不产生 `target/` 产物或只读元数据的命令，例如 `cargo fmt`、`cargo fmt --check`、`git diff` / `git status` / `git log`、`gh run watch` / `gh run list`、纯文本/脚本验证、编辑器内 rust-analyzer 的查询（rust-analyzer 自动管理自己的小 `target/`，如占满磁盘需立即在编辑器关掉并 `rm -rf target`）。
- **本地编译产物的唯一来源**：在任务分支上 push 一次，`.github/workflows/dev_build.yml` 跑出 `warp-oss-linux-x64` artifact，本地用 `gh run download` 拉下来直接 `./warp-oss` 验证。
- **禁止行为**：本地 `cargo build`、本地 `cargo run`、本地 `cargo test --no-run`、本地 `cargo install`、本地安装 Linux GUI 系统依赖（ALSA / OpenSSL / Freetype / Fontconfig / libgit2 / libclang）做编译用途、把 `target/` 提交进 Git（仓库已有 `.gitignore` 兜底，必要时手工 `echo target >> .gitignore`）。
- **唯一例外**：纯文本/配置/文档改动（只动 `.md` / `.yml` / `.toml` / 注释），不需要编译也不需要拉 artifact；但 `.rs` 改动仍必须走 Actions 编译验证。

### 为什么要这样做

- 本地编译 5-15 分钟才拿到一个跟 CI 等价的产物（同一份 `Cargo.lock` 哈希走 `Swatinem/rust-cache` 缓存），避免本地每次 `cargo clean` 之后再全量编一遍。
- 大机器 `ubuntu-latest-large` 默认并行 16 个 rustc，编译时间通常是本机 4 核 `-j 4` 的 1/3 ~ 1/5，且不会卡死笔记本。
- Actions 的 `Swatinem/rust-cache` 按 `Cargo.lock` + target 哈希分桶，只要不手动改 `Cargo.toml` 大版本，第一次跑会冷编译、之后命中 master 缓存。任务分支的缓存策略：只读 master 缓存（不覆写），不会污染 master cache storage。

### 任务分支 vs 主分支的纪律

`AGENTS.md` 的所有开发流程都建立在这个区分之上：

- **主分支** `master`：
  - 只接受合并，不接受直接 push。
  - merge 后由 `populate_build_cache.yml` + `ci.yml` 维护缓存。
  - 不允许在 `master` 上跑 `dev_build.yml`（dev_build 的 `branches-ignore: [master]` 显式排除）。
- **任务分支**：
  - 命名严格遵守 `feature|fix|docs|chore/<scope>`。
  - 每 push 一次自动触发 `dev_build.yml`。
  - 通过 `gh run download` 拉取 artifact 验收。
  - 验收通过后**通过 PR**合并回 `master`。
- **任务分支生命周期**：
  1. 从最新 `master` 切出。
  2. 在该分支上开发、commit、push，**每 push 一次自动触发** `dev_build.yml`，产出最新 `warp-oss-linux-x64` artifact。
  3. 用户在 Actions 页面或 `gh run download` 拉 artifact 验收。
  4. 验收通过后，任务分支通过 PR 合并回 `master`。
  5. 合并后保留任务分支若干天，便于追溯；清理时机由用户决定。

### dev_build.yml：本地验证的标准流水线

`.github/workflows/dev_build.yml` 是**任务分支唯一的本地验证编译入口**，请按以下规范使用：

- **触发时机**：每次 push 到非 `master` 分支自动跑；用户也可以在 Actions 页面手动 `Run workflow`。
- **不触发**的情况：push 到 `master`（master 走 `ci.yml` + `beta_release.yml`）、纯文档/`.toml`/`.yml` 改动（默认仍会跑，但只产空 artifact 视情况而定——如果纯文档改动，CI 跑通即可，**不必**下载 artifact）。
- **Runner**：`ubuntu-latest-large`（GitHub 托管、16C/64G），**不再使用本机编译**。
- **缓存**：复用 `.github/actions/prepare_environment` 里的 `Swatinem/rust-cache`，key 沿用 `linux`，**`save-if: github.ref == 'refs/heads/master'`**——任务分支只读 master 缓存、不写回，避免污染 master cache storage。
- **产物**：
  - 单个 artifact：`warp-oss-linux-x64`（tar.gz，内含 `warp-oss` 二进制 + `BUILD_SHA` + `BUILD_REF` 两个标记文件，便于把下载下来的产物对应回 git 提交）。
  - 保留期：14 天。
  - 不做 deb / rpm / AppImage 打包，保持"0 打包"——本地验证只需要一个能直接执行的二进制。
- **何时需要下载 artifact**：
  - `.rs`、`.toml` 改动且需要看效果。
  - 涉及 UI、shader、字体、icon、warpui、CLI 子命令、agent 行为等任何需要肉眼验证的功能。
  - 涉及多平台差异逻辑（Windows/Linux 行为分支）—— 本地只跑 Linux artifact，Windows 验证另走 `beta_release.yml`。
- **何时不需要下载 artifact**：
  - 纯 `.md`、`.yml`、`.json`、注释改动。
  - 改动只影响 `cargo fmt` / `cargo clippy` / 纯逻辑的 Rust 模块——但仍需要 CI 跑通作为证据。

### 本地拉取 artifact 的标准命令

任务分支 push 完成后，开发者在本机用以下命令拉取并运行：

```bash
# 1. 查看最近一次 dev_build 运行
gh run list --workflow=dev_build.yml --branch="$(git rev-parse --abbrev-ref HEAD)" \
  --json databaseId,headSha,status,conclusion,createdAt --limit 5

# 2. 下载最新一次成功运行的 artifact 到当前目录的 .dev-build/ 目录
mkdir -p .dev-build
gh run download \
  --workflow=dev_build.yml \
  --branch="$(git rev-parse --abbrev-ref HEAD)" \
  --name warp-oss-linux-x64 \
  --dir .dev-build

# 3. 解压并运行
tar -xzf .dev-build/warp-oss-linux-x64.tar.gz -C .dev-build
./.dev-build/warp-oss --version       # 快速 sanity check
./.dev-build/warp-oss                  # 启动 GUI（X11 / XWayland）

# 4. 验证完按需清理
rm -rf .dev-build
```

补充说明：

- `gh run download` 默认拉到当前目录，**不会**污染 `target/`。
- `warp-oss` 是动态链接少数系统库的二进制，直接 `./.dev-build/warp-oss` 即可；少数情况下缺 `.so`，用 `ldd .dev-build/warp-oss` 查看并按需补包。
- 如果下载失败，先 `gh auth status` 确认登录的是 fork 仓库（`Mi0uno/Agentwarp`）的 GitHub 账号；fork 仓库的 workflow artifact 默认只有 push 过该分支的成员能下载。
- 也可以用 GitHub Web UI 打开 `Actions → Dev Build (Linux x64 GUI) → 选 run → 底部 Artifacts → 下载**整个 tar.gz**`，但 `gh` CLI 更适合反复迭代。
- 任务分支 push 触发 workflow 的**冷启动耗时**（Actions queue + checkout + LFS pull）通常 1-3 分钟；首次冷编译 5-15 分钟；命中缓存后 2-5 分钟出 artifact。

### 本地仍然允许的轻量检查

虽然不编译，但开发期间仍可（且建议）跑以下不产生 `target/` 重产物的轻量检查：

- `cargo fmt --check` — 仅读源文件、写极少缓存，不产生完整 `target/`。
- `cargo fmt` — 自动格式化后再 commit。
- `git diff` / `git status` / `git log` — git 元数据。
- `gh run watch` / `gh run list` — 看 Actions 实时状态。
- 静态阅读源码、`rg` 搜索、`sed`/`awk` 文本处理、纯脚本验证（不调 rustc）。
- 编辑器内的 rust-analyzer（rust-analyzer 自身会建一个 `target/` 子集，但**通过 LSP/IDE 自动管理**，不会无限增长；如发现它占满磁盘，立刻在编辑器里关掉 rust-analyzer 然后 `rm -rf target`）。

**禁止**的本地命令（即便轻量也禁止）：

- `cargo build` / `cargo build --release` / `cargo test --no-run` — 写完整 `target/`，占满磁盘。
- `cargo run` / `cargo bench` — 同上。
- `cargo install ...` — 在 `~/.cargo/bin` 装二进制，本机磁盘同样吃紧；改用 `cargo binstall` 走预编译，或干脆不装。
- `cargo nextest run` — 编译测试目标，等同于 `cargo test`。
- 任何 `--release` / `--profile=*` / `-p <workspace-crate>` 跨多 crate 的全量 cargo 调用。

### 例外：必须本地编译的极少数情况

以下场景**必须先得到用户口头确认**才能在本地编译：

- 需要在本地用 gdb/lldb 断点调试 Actions 跑出来的 panic——这种情况下用 `cargo build --release` + debug symbols。
- Actions 跑通但本地有运行时问题（X11 / Wayland / GPU / 文件系统差异），需要本地复现。
- 紧急修复需要跳过 Actions 队列快速验证（必须有充分理由，并在交付汇报里写明"本轮未走 Actions，原因：..."）。

每次例外都必须在交付汇报里写明"本轮偏离了默认 '本地不编译' 策略，理由是：..."。

### 不变的跨平台要求

即便本地不再编译，**每个功能点仍然必须同时考虑 Linux 和 Windows 适配**，引入只在单一平台成立的路径、命令、依赖、权限、换行、终端、窗口系统或 shell 假设是禁止的：

- 涉及 Windows 平台差异的改动：本地通过 `dev_build.yml` 跑通 Linux artifact **还不够**，还必须额外触发 `.github/workflows/beta_release.yml`（push 临时 `v*-beta*` tag 或 `workflow_dispatch`）跑 `windows_installer`（x64 + arm64）job。
- 不涉及平台差异的改动（纯 Linux 路径、纯逻辑、纯 .md）：只跑 `dev_build.yml` 即可。
- 如果改动需要平台条件分支，必须分别说明 Linux 与 Windows 的行为。

### 清理与缓存策略

- **本地**不再需要 `cargo clean`——因为本地不再编译，`target/` 永远不应该在本地出现。如果在 `target/` 看到任何文件，说明上一轮被 stash 的工作树或 rust-analyzer 残留，立即 `rm -rf target` 清理。
- **Actions 端**：`Swatinem/rust-cache` 自动管理（key = `linux`），不需要手动干预。任务分支只读不写。
- **开发者下载的 artifact**：每次验证完用 `rm -rf .dev-build` 清理，不占用磁盘。

### 交付汇报要求

每次交付汇报必须包含（与之前一致，新增项加 ⭐）：

- 任务分支名（必须是 `feature|fix|docs|chore/<scope>` 之一）。
- 主分支（`master`）合并结果。
- 最终 commit SHA。
- ⭐ **本地拉取/运行的 artifact 名**（`warp-oss-linux-x64`，含 `BUILD_SHA`）和对应 `gh run download` 命令。
- ⭐ **GitHub Actions dev_build run URL**（便于回看日志）。
- 已触发的跨平台 GitHub Actions Windows 构建结果（如适用，附 `windows_installer` x64 + arm64 链接或日志）。
- 完整测试流程结果（`dev_build.yml` + 相关 `ci.yml` job）。
- ⭐ 本轮**未走本地编译**的证据（`target/` 不存在或大小 < 1M）。
- 缓存清理结果（`.dev-build/` 已删除）。
- 剩余风险。

## 推荐交付顺序

1. 明确需求和影响范围。
2. 制定开发计划并询问用户确认。
3. 从最新 `master` 切出独立任务分支（`feature|fix|docs|chore/<scope>`）。
4. 实施改动。
5. 任务分支 push，触发 `.github/workflows/dev_build.yml` 跑 Linux GUI 编译（无需本地编译）。
6. 用 `gh run download` 拉取 `warp-oss-linux-x64` artifact，本地 `./warp-oss` 验证。
7. 涉及平台差异的改动额外 push 临时 `v*-beta*` tag 或在 Actions 页面 `workflow_dispatch` 触发 `beta_release.yml`，等 `windows_installer`（x64 + arm64）全部成功。
8. 在任务分支下用完整测试流程覆盖：format check、clippy（如适用）、dev_build run 通过、相关单元测试、相关集成或端到端验证、手工验收。
9. 主动询问用户是否测试通过。
10. 用户确认通过后，按 PR 流程把任务分支合并回 `master`。
11. 清理本地 `.dev-build/` 目录，确认 `target/` 不存在。
12. 汇报任务分支名、主分支合并结果、最终 commit SHA、`warp-oss-linux-x64` artifact BUILD_SHA、dev_build run URL、Windows 构建结果（如适用）、测试结果、缓存清理结果和剩余风险。

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
- 2026-06-14 复盘：再跑一次 `cargo check`（默认 -j16）把机器卡到无响应，意识到本机 16C/15G/93% 磁盘余量无法承受 cargo 全量并行；补"本地编译资源阈值"小节强制 `-j 2` + `cargo clean`。
- 2026-06-14 复盘：用户进一步要求**所有本地编译交给 GitHub Actions**；本机完全不再 `cargo build`/`cargo run`，改由 `.github/workflows/dev_build.yml` 跑 `ubuntu-latest-large` 产出 `warp-oss-linux-x64` artifact，本地 `gh run download` 拉取直接运行；同步重写"本地编译"章节为"本地编译全面交给 GitHub Actions"，新增任务分支 vs 主分支纪律、`dev_build.yml` 触发/产物规范、本地 `gh` 拉取命令模板、清理策略与交付汇报模板。
- 不要在仓库文档、提交记录或脚本中记录 sudo 密码、token、SSH key 等敏感信息。
