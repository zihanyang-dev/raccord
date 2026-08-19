# Raccord 0–10 Roadmap

**定位：** Agent-native、headless、服务端优先、确定性的媒体后期运行时。

这份路线图是执行顺序，不是功能愿望清单。每一级必须满足验收条件后，才进入下一级；实验能力可以先存在 `experiments/`，但正式能力必须迁移到 `crates/`。

## 0. 基线冻结 — 已完成

- Rust workspace、基础 crates、CI 和代码风格。
- 语义 `find / inspect / plan_edit / commit_edit / verify` 协议。
- 结构化事务错误和 revision 校验。
- Pi benchmark、短时间线和 48 秒长时间线。
- `ArtifactStore`：原子发布、manifest 校验、RAII key lock、stale lock recovery。

**验收：** workspace 与 product-path experiment 的 check、test、clippy 全部通过。

## 1. Canonical semantic core — 当前阶段

把实验中已经验证的语义模型迁移到正式 crates：

- `ClipId`、`SourceRef`、时间范围和 anchor。
- transition 语义与相邻 clip 校验。
- 不允许 Agent 直接提交绝对帧号或 FFmpeg filtergraph。
- 为正式 timeline 类型补充错误模型和单元测试。

**验收：** transition、anchor、duration、adjacency 的正式 API 测试通过；实验行为不回归。

## 2. Transactional runtime — 进行中

正式 `raccord-runtime` 现在拥有 `Revision`、`PlanToken`、`EditPayload`、`plan_edit_at`、`verify`、`commit` 和 `commit_with_store`，只允许通过验证的 plan 提交，并拒绝 stale revision、非法 semantic edit、非法 render unit 与 token replay。当前 payload 已承接 ripple-delete、source replacement、trim、insert、move、marker、subtitle 和 transition，并会直接影响 render plan；`FileRevisionStore` 已提供原子 revision 持久化和恢复。Stage 2 剩余工作是把 command/result/error schema 序列化后接入 server。

建立正式 runtime 的事务边界：

- immutable project revision；
- plan token / commit；
- stale revision 拒绝；
- verify 作为提交后的不变量检查；
- 可序列化的 command/result/error schema。

**验收：** 同一事务在重复执行、过期 revision、非法 anchor 和非法 duration 下都得到确定性结果。

## 3. Semantic render planning and cache graph

把缓存失效边界从实验迁移到正式 planner：

- clip media artifact；
- subtitle overlay artifact；
- transition composite artifact；
- metadata-only artifact；
- 依赖图和 cache key provenance。

**验收：** marker-only、subtitle-only、audio-gain-only、transition-only 修改分别只失效预期节点。

## 4. FFmpeg adapter and renderer contract

建立正式 renderer adapter，不让业务层拼接 filtergraph：

- typed render requests；
- FFmpeg command builder 仅在 adapter 内部；
- ffprobe 结果结构化解析；
- artifact 发布直接接入 `ArtifactStore`；
- CPU/reference path 保留。

**验收：** 48 秒 fixture 完成全量和局部渲染，输出 metadata、时长、帧数可验证。

## 5. Renderer worker

实现服务端 renderer worker：

- job queue；
- 状态机：queued / running / succeeded / failed / cancelled；
- 同 key 去重；
- timeout、子进程回收、日志和 exit reason；
- 崩溃后可恢复的 job 状态。

**验收：** 并发任务、重复任务、超时、取消和 worker 崩溃测试通过。

## 6. Project persistence and server API

把 runtime 接入正式 server：

- project store；
- revision persistence；
- semantic command API；
- render job API；
- event/status stream；
- artifact download/reference API。

**验收：** server 重启后项目 revision、job 状态和已发布 artifact 可恢复。

## 7. Concurrency, isolation, and policy

完成生产级边界：

- project-scoped locks；
- tenant/project isolation；
- workspace permission checks；
- resource limits；
- cancellation propagation；
- path traversal 和输入文件校验。

**验收：** 跨项目访问、非法路径、并发 commit、超额资源和取消竞态均被拒绝或安全收敛。

## 8. Performance and incremental economics

用固定 fixture 测量真实收益：

- full render vs partial render wall time；
- CPU、内存、磁盘读写；
- cache hit ratio；
- artifact reuse ratio；
- 并发吞吐和队列延迟。

**验收：** 生成可重复 benchmark report，所有指标带 fixture、机器和 renderer 版本。

## 9. Operational hardening

补齐长期运行能力：

- cache GC / TTL / quota；
- manifest 和 artifact reconciliation；
- structured logs / metrics / tracing；
- retry policy 和 failure classification；
- 字幕字体、样式和多 cue；
- 更完整的多轨道与音频测试。

**验收：** 长时间运行、磁盘压力、部分写入、重启恢复和字体缺失场景均可诊断且不破坏项目状态。

## 10. Production readiness

进入可部署产品阶段：

- 稳定 API/versioning；
- migration policy；
- security review；
- deployment images；
- operational runbook；
- compatibility matrix；
- release gates 和 rollback strategy。

**最终验收：** 新环境可从零部署；Agent 只能使用语义 API；任务可取消、可恢复、可观测；相同输入、版本和策略得到确定性结果。

## 执行规则

1. 每次只推进一个主阶段，但连续完成该阶段的代码、测试和验证，不停在半成品设计。
2. 实验脚本只用于验证假设；通过后立即迁移到正式 crate。
3. 任何新增 renderer 能力必须先定义语义输入、依赖边界、缓存边界和错误模型。
4. `cargo fmt --check`、`cargo test`、`cargo clippy -D warnings` 和定向 LSP 是每阶段的最低门槛。
5. 性能、并发、安全和恢复不能推迟到最后才首次验证。
