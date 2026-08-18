# Raccord Rust Style Guide

## Status

Draft / Engineering Constitution v0.1

这份文档定义 Raccord 如何写出可读、可维护、可验证的 Rust。它关注的不只是格式，而是：

```text
所有权是否清晰
状态转换是否可见
时间单位是否安全
失败是否可诊断
资源是否有边界
结果是否可复现
```

本指南受 Rust 官方风格、Rust API Guidelines 以及 Onmark 的工程实践启发，但它是 Raccord 自己的规范，不要求复制其他项目的 crate 划分或产品架构。

---

## 1. 三层风格

不要混淆三个概念：

### 1.1 格式

交给 `rustfmt`。不要手工对齐字段、参数或 match 分支。

### 1.2 Idiomatic Rust

关注：

- ownership；
- borrowing；
- `Result` / `Option`；
- enums；
- typestate；
- traits；
- API 设计；
- resource lifetime。

### 1.3 Raccord 工程规则

关注媒体系统自己的正确性：

- 时间必须精确；
- 编译阶段必须可区分；
- render plan 必须可复现；
- frame/audio buffer 必须有上限；
- subprocess 必须可取消和清理；
- cache identity 必须完整；
- authored diagnostics 与 infrastructure errors 必须分开。

格式正确不等于工程正确。

---

## 2. 总体审美：矩形函数、树形模块、线性管线

Raccord 的代码应具有清晰的矩形轮廓：

```rust
pub async fn render(request: RenderRequest) -> Result<RenderOutput, RenderError> {
    let project = load_project(&request).await?;
    let resolved = resolve_project(project)?;
    let plan = build_render_plan(resolved)?;
    let artifacts = execute_plan(plan).await?;

    assemble_output(artifacts).await
}
```

函数应该让读者一眼看到阶段：

```text
load → resolve → plan → execute → assemble
```

避免深层嵌套：

```rust
for clip in clips {
    if clip.enabled() {
        if let Some(source) = sources.get(clip.source_id()) {
            match probe(source) {
                Ok(metadata) => {
                    if metadata.duration > Duration::ZERO {
                        plans.push(build_plan(clip, metadata)?);
                    }
                }
                Err(error) => diagnostics.push(error.into()),
            }
        }
    }
}
```

优先使用边界处的 early return / `let ... else`：

```rust
for clip in clips.enabled() {
    let Some(source) = sources.get(clip.source_id()) else {
        diagnostics.push(missing_source(clip));
        continue;
    };

    let metadata = match probe(source) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(probe_failed(clip, error));
            continue;
        }
    };

    if metadata.duration.is_zero() {
        diagnostics.push(empty_source(clip));
        continue;
    }

    plans.push(build_plan(clip, metadata)?);
}
```

模块应该形成树，而不是散落的工具函数：

```text
raccord-core
└── timeline
    ├── anchors
    ├── placement
    └── ripple
```

禁止创建没有领域归属的：

```text
utils
common
shared
misc
```

---

## 3. 类型就是管线

每个阶段产生不同的类型，不能让一个可变结构贯穿所有阶段：

```rust
pub fn parse(source: &SourceDocument) -> ParseReport<ParsedProject>;

pub fn link(
    parsed: ParsedProject,
    assets: &AssetCatalog,
) -> ResolveReport<LinkedProject>;

pub fn resolve(linked: LinkedProject) -> ResolveReport<ResolvedProject>;

pub fn plan(resolved: ResolvedProject) -> RenderPlan;
```

目标是：

```text
ParsedProject 不能直接 render
LinkedProject 不能包含未解析的 asset
ResolvedProject 不能包含未解决的 anchor
RenderPlan 不能包含未解决的时间或资源
```

如果一个状态在运行时才检查，应该先问：

> 能不能让类型系统阻止这个状态被构造？

不要为了形式而 typestate。只有状态差异会阻止真实误用时才使用不同类型；否则使用清晰的 enum。

---

## 4. 时间和媒体单位必须使用 Newtype

禁止用裸数字表达不同语义：

```rust
fn render(start: u64, duration: u64, rate: f64);
```

优先使用：

```rust
pub struct FrameIndex(u64);
pub struct FrameCount(u64);
pub struct SampleIndex(u64);
pub struct TimelineTime(RationalTime);
pub struct SourceTime(RationalTime);
pub struct ClipId(String);
pub struct ContentHash([u8; 32]);
```

这些类型即使底层表示相同，也不能隐式互换。

禁止：

```rust
let frame = (seconds * fps as f64) as u64;
```

优先：

```rust
let frame = timebase.frame_at(timestamp, Rounding::Floor)?;
```

所有时间转换和 rounding policy 集中在 `raccord-time`，不能散落在 compositor、audio、CLI 和 adapter 中。

音频使用 sample index；视频使用 rational frame/time。毫秒和浮点秒只能作为输入输出边界格式，不能作为内部时间真相。

---

## 5. Parse once, then trust the type

外部文本只在边界解析一次：

```rust
pub struct CueName(String);

impl TryFrom<&str> for CueName {
    type Error = InvalidCueName;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value
            .strip_prefix("cue:")
            .filter(|name| !name.is_empty())
            .map(|name| Self(name.to_owned()))
            .ok_or(InvalidCueName)
    }
}
```

后续函数接受 `&CueName`，不要再次接受 `&str` 并重新验证。

同样的规则适用于：

```text
AssetUri
FrameRate
ColorSpace
ChannelLayout
ArtifactDigest
CapabilityId
```

无效状态应当无法构造，而不是每个函数重复检查。

---

## 6. Enum 优先于布尔和互斥 Option

避免：

```rust
render(frame, true, false, true)?;
```

使用有意义的类型：

```rust
render(
    frame,
    RenderOptions {
        capture: CaptureMode::BeginFrame,
        alpha: AlphaMode::Opaque,
        retry: RetryLimit::new(3),
    },
)?;
```

避免互相冲突的 Option：

```rust
struct Timing {
    duration: Option<Duration>,
    cue: Option<CueId>,
    voice_over: Option<AssetId>,
}
```

优先：

```rust
enum Timing {
    Fixed(Duration),
    Until(CueId),
    FromVoiceOver(AssetId),
}
```

闭合的领域 enum 应该 exhaustive match。只有明确支持未来未知值的 wire enum 才使用 `#[non_exhaustive]` 或显式 unknown variant。

---

## 7. Ownership 和资源生命周期

### 7.1 长期 ownership 形成树

- 长期拥有者使用 owned 类型；
- 只观察数据时使用 `&str`、`&Path`、slice；
- 跨任务共享只读数据时才使用 `Arc<T>`；
- 不通过 clone 掩盖 ownership 问题；
- 不把 lock guard 暴露到 public API；
- 大型 frame/audio buffer 的 clone 必须是明确、可测量的管线决策。

### 7.2 RAII 负责清理

文件、临时目录、FFmpeg、浏览器、encoder、permit、tracing span 都是资源。

资源类型应该拥有清理责任：

```rust
let mut encoder = EncoderProcess::spawn(config).await?;
let render_result = render_frames(&mut encoder).await;
let shutdown_result = encoder.shutdown().await;

render_result?;
shutdown_result?;
```

`Drop` 可以作为强制终止的最后保险，但不能隐藏调用方必须知道的异步清理失败。

### 7.3 共享状态的默认方案不是 `Arc<Mutex<_>>`

优先：

```text
一个 task 拥有一个资源
通过 bounded channel 传递命令
```

```rust
let (commands, inbox) = tokio::sync::mpsc::channel::<EncoderCommand>(8);
tokio::spawn(run_encoder(inbox, encoder));
```

每个 channel 必须有：

```text
capacity
backpressure policy
owner
cancellation behavior
```

禁止无限 task、无限 channel 和跨 `.await` 持有 mutex guard。

---

## 8. Errors 与 Diagnostics

用户写错项目是正常输入，不应与系统故障混为一谈。

### 8.1 Authored diagnostics

```rust
pub struct CompileReport {
    pub plan: Option<RenderPlan>,
    pub diagnostics: Vec<Diagnostic>,
}
```

例如：

```text
UNKNOWN_CLIP
INVALID_TRANSITION_RANGE
UNRESOLVED_ANCHOR
OVERLAPPING_VIDEO
MISSING_SOURCE_REFERENCE
```

可以聚合多个诊断后返回。

### 8.2 Machinery errors

基础设施故障使用 typed error：

```text
FFmpeg_CRASHED
IPC_PROTOCOL_ERROR
WORKER_TIMEOUT
ASSET_IO_FAILED
CACHE_COMMIT_FAILED
RESOURCE_EXHAUSTED
```

库 API 不使用 `Box<dyn Error>` 抹掉语义，也不使用字符串作为稳定错误协议。第三方错误在拥有依赖的边界翻译成 Raccord error。

### 8.3 `unwrap` 规则

生产代码中的 `expect` 只能用于本地已经建立的不可违反 invariant，并且消息必须说明 invariant。用户输入错误、文件损坏、worker 失败都不能使用 panic。

---

## 9. 注释与 Rustdoc

Raccord 需要注释规范，但注释不能替代类型、命名和清晰的控制流。

### 9.1 注释写什么

注释应该解释代码本身无法表达的内容：

- 为什么选择这个算法或边界；
- 一个 invariant 如何被建立和维护；
- ownership、生命周期或并发约束；
- 时间 rounding、缓存 key、分片策略等协议决定；
- 第三方 runtime 的限制和 workaround；
- 一个看似奇怪的安全或性能取舍。

不要写只重复代码的注释：

```rust
// Increment frame by one.
frame += 1;
```

应当写清楚原因：

```rust
// Advance on the output grid so the exclusive end never emits a duplicate frame.
frame = frame.next_output_frame();
```

### 9.2 Rustdoc 层级

- `///`：公开类型、方法、trait 和稳定协议；说明用途、输入语义和返回值；
- `//!`：非平凡 module 或 crate；说明职责、边界和主要 invariant；
- `//`：局部实现细节、算法原因和短期控制流说明；
- `// SAFETY:`：每一个 unsafe block 必须说明维护的安全条件；
- `// TODO:`：只有对应 issue 或明确后续任务时才允许使用，不能作为未完成设计的垃圾桶。

公开 API 的 Rustdoc 应在需要时包含：

```text
# Examples
# Errors
# Panics
# Safety
```

### 9.3 注释语言和生命周期

代码注释、identifier、public Rustdoc 使用 English；架构、设计和用户文档可以使用中文。注释必须随着代码一起更新，不能保留已经失效的协议描述。

---

## 10. Traits 只代表真实边界

不要给每个 struct 都包一层 trait：

```rust
trait TimelineSolver {
    fn solve(&self, project: Project) -> Result<Plan, Error>;
}

struct TimelineSolverImpl;
```

如果只有一个实现，直接使用具体类型：

```rust
pub struct Solver {
    policy: SolvePolicy,
}
```

Trait 应该出现在真正变化的边界：

```rust
trait AssetStore
trait FrameSource
trait ArtifactStore
trait ProcessRunner
trait MediaActionProvider
```

引入 trait 至少应满足一项：

- 已经有两个真实实现；
- 运行时确实需要选择实现；
- 测试需要替换外部边界；
- 它是对外公开的稳定扩展协议。

默认使用静态 dispatch；只有运行时选择实现时才使用 `dyn Trait`。

---

## 11. Core、adapter 和 server 的依赖方向

依赖必须向纯核心收敛：

```text
raccord-time
    ↓
raccord-ir
    ↓
raccord-timeline / raccord-constraints / raccord-media
    ↓
raccord-audio / raccord-compositor
    ↓
raccord-planner
    ↓
raccord-cache / raccord-rmap
    ↓
raccord-runtime
    ↓
raccord-server / raccord-cli
```

更具体地说：

- `raccord-time`、`raccord-ir` 不依赖 filesystem、network、FFmpeg、GPU、Chromium、cloud SDK；
- `raccord-media` 可以使用 probing 和媒体库，但不把第三方类型泄漏进 IR；
- `raccord-audio`、`raccord-compositor`、`raccord-planner` 只表达媒体计算和执行计划，不拥有 server 生命周期；
- `raccord-runtime` 拥有 worker、subprocess、取消和重试；
- `raccord-server` 拥有 API、队列、存储和 worker orchestration；
- `raccord-cli` 是 composition root；
- AWS、Lambda、数据库、云队列类型不能进入纯核心 crate；
- `utils/common/shared` 不作为跨领域 dumping ground。

`main.rs` 应该像进程结构图：

```rust
let config = Config::load()?
    .validate()?;
let store = build_store(&config)?;
let runtime = build_runtime(&config, store.clone())?;

run_command(runtime).await
```

资源和依赖的构造位置必须可见，禁止 service locator、隐藏 singleton 和全局可变 registry。

---

## 12. Determinism

产生持久化数据、cache key 或 diagnostics 时：

- 不依赖 `HashMap` 遍历顺序；
- 显式排序或使用有序结构；
- 不在纯编译阶段读取 wall clock、locale、timezone、环境变量或随机数；
- 随机数必须有 seed，并进入 plan hash；
- canonical serialization 只有一个实现；
- cache key 包含所有真正影响输出的输入；
- 等价输入产生稳定 IR、稳定 diagnostics 和稳定 plan bytes。

等价任务必须满足：

```text
same input
+ same plan
+ same execution lock
→ same artifact identity
```

不确定性不能被隐藏在缓存系统里。

---

## 13. Async、subprocess 和并发

- 所有 queue 有容量；
- 所有 worker 有资源上限；
- CPU-heavy work 不运行在 async executor 上；
- cancellation 必须传播到 task 和子进程；
- FFmpeg/Chromium 使用参数数组，不使用 shell string；
- stdout/stderr 有大小限制；
- process tree 必须可清理；
- 临时输出必须与最终 artifact 分离；
- 最终结果必须 atomic commit。

Render pipeline 的 backpressure 必须回传给 frame producer，不能因为 encoder 变慢而无限积累帧。

---

## 14. 测试和 Definition of Done

测试优先验证产品协议，不验证私有实现细节：

```text
source → parsed model
source + assets → resolved timeline
invalid source → stable diagnostics
resolved timeline → stable render plan
render plan → deterministic artifact
```

推荐测试层级：

- unit test：纯时间、区间、约束和转换；
- property test：时间代数、区间关系、canonicalization；
- golden test：IR、diagnostics、plan bytes；
- integration test：FFmpeg、worker、IPC、取消、清理；
- benchmark：代表性视频、音频和缓存场景；
- conformance fixture：外部 Media Action integration。

修 bug 的第一步应该是新增一个失败的 focused test 或 fixture。

一个 Rust 改动只有在以下条件满足后才算完成：

```text
fmt
check
clippy
unit tests
relevant integration/conformance tests
public API/documentation
dependency direction
resource cleanup
```

---

## 15. Toolchain 与 lint

Raccord 使用 mise 固定 Rust toolchain 和 MSRV，不依赖机器上的任意 stable：

```toml
# mise.toml
[tools]
rust = "1.97.0"
```

运行命令时使用当前 mise 环境：

```bash
mise install
mise exec -- cargo check --workspace
mise exec -- cargo test --workspace
```

格式交给 rustfmt。Lint 应该高信号、分 crate 配置，不要把所有 Clippy `pedantic` 或 `restriction` 规则无差别升级为硬错误。

纯核心 crate 默认：

```rust
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::print_stdout, clippy::print_stderr)]
```

如果未来确实需要 unsafe：

1. 放到独立 adapter 或专门 module；
2. 暴露 safe API；
3. 为每个 unsafe block 写 `// SAFETY:`；
4. 添加边界、并发和 sanitizer/Miri 测试；
5. 记录架构例外。

---

## 16. Code Review 反模式

以下代码默认需要修改或解释：

```text
裸 f64 表达 timeline truth
u64/String 在不同领域语义间直接传递
一个可变 struct 表示所有编译阶段
每个 struct 都创建 trait
为了通过 borrow checker 到处 clone
Arc<Mutex<_>> 作为默认架构
无限 channel 或无限 spawn
锁跨 await
shell string 启动 FFmpeg/Chromium
serde_json::Value 穿透整个核心
HashMap 顺序进入持久化数据
panic/unwrap 处理用户输入
utils/common/shared dumping ground
没有 benchmark 的“性能优化”
```

---

## 17. Raccord 的一句话代码审美

> **让类型表达阶段，让所有权表达生命周期，让错误表达责任，让模块表达变化边界，让函数表达线性管线。**
