# Raccord 总体架构设计

**状态：Draft / Architecture Proposal v0.1**  
**项目：Raccord**  
**定位：Agent-native, headless, deterministic media post-production runtime**

---

## 1. 摘要

Raccord 是一个面向 Agent、服务端优先、无界面的确定性媒体后期制作运行时。

它负责：

- 语义时间线编辑；
- 视频剪辑与合成；
- 音频混音；
- 字幕；
- 基础调色；
- 转场与图层；
- 增量渲染；
- 并发任务调度；
- 最终编码和交付。

Raccord 不负责：

- 训练或调度视频生成模型；
- 规定 Hyperframes、Remotion、Blender、After Effects 等系统的内部 DSL；
- 把 100 个外部系统强行翻译成一个万能场景图；
- 让 Agent 直接编写 FFmpeg filtergraph；
- 让 Agent 手工计算帧号、轨道和音频 sample；
- 在 v1 建设公共插件市场。

Raccord 的核心原则是：

> **Raccord 统一后期语义和执行边界，不统一所有外部创作系统的内部语言。**

外部系统保留自己的 DSL 和运行时，通过统一的媒体动作协议接入 Raccord。Agent 面对的是少量语义操作，而不是 100 套插件 API。

---

## 2. 设计目标

### 2.1 Agent 目标

Agent 应该表达：

```text
把第二个镜头放到旁白第一句之后
删除这段并 ripple 后续内容
让音乐在说话时降低 8dB
给这段加字幕
用 Remotion 做一个动态标题
```

Agent 不应该直接表达：

```text
track = 3
start = 127
end = 241
filter_complex = "..."
```

Raccord 负责：

- 解析语义锚点；
- 计算时间；
- 解决轨道和范围冲突；
- 处理 ripple；
- 维护 A/V 同步；
- 规划局部重渲染；
- 验证最终结果。

### 2.2 服务端目标

- headless；
- CPU 可运行；
- GPU 可选；
- 支持本地、容器、远程 worker；
- 可取消、可重试、可观测；
- 可分片渲染；
- 能复用缓存；
- 结果可复现；
- 外部执行环境崩溃不影响核心服务。

### 2.3 编辑目标

项目修改应当是：

```text
小型语义事务
    ↓
局部依赖分析
    ↓
局部失效
    ↓
局部渲染
    ↓
复用未受影响缓存
```

而不是每次修改都重新生成完整项目、完整 filtergraph 和完整视频。

---

## 3. 核心架构

```text
┌──────────────────────────────────────────────┐
│              Agent / Client Layer             │
│       HTTP / RPC / CLI / SDK / UI             │
└──────────────────────┬───────────────────────┘
                       │ semantic commands
                       ▼
┌──────────────────────────────────────────────┐
│                Raccord Host                   │
│ project store / agent tools / jobs / policies │
└──────────────────────┬───────────────────────┘
                       │ commands + project revision
                       ▼
┌──────────────────────────────────────────────┐
│              Raccord Core Engine              │
│ IR / time / solver / audio / compositor       │
│ planner / cache / renderer / encoder          │
└──────────────┬─────────────────┬─────────────┘
               │                 │
               ▼                 ▼
       Built-in Media Ops   External Media Actions
               │                 │
               │          Integration Capsules
               │       Remotion / Blender / HF / AE
               ▼                 ▼
          Core artifacts ← validated artifacts
                       │
                       ▼
                 Final output
```

### 3.1 Raccord Core Engine

Core 是 Raccord 的产品本体，拥有完整的后期语义：

```text
Project IR
Semantic Timeline
Time Resolver
Constraint Solver
Composition Graph
Audio Graph
Caption Model
Color Model
Render Planner
Incremental Cache
CPU/GPU Execution
Final Encoder
```

Core 不导入：

```text
HTML
GSAP
Chromium
Blender API
Remotion API
After Effects API
AWS Lambda API
```

Core 只依赖稳定的内部数据结构和外部执行协议。

### 3.2 Raccord Host

Host 是服务端产品层，负责：

- 项目持久化；
- Agent API；
- command 注册；
- capability/integration 注册；
- render job 管理；
- worker 生命周期；
- 权限策略；
- 配置和认证；
- 任务状态和事件流。

Host 可以拥有外部能力，但不能让外部能力绕过 Core 直接修改项目内部状态。

### 3.3 Media Runtime

底层媒体运行时可以使用：

```text
FFmpeg
GStreamer
Rust native code
CPU compositor
GPU compositor
hardware decoder/encoder
```

这些是实现手段，不是项目 IR，也不是 Agent API。

---

## 4. 持久化模型：Project IR

Raccord 使用自己的 Canonical Project IR。

它不是：

- HTML 文件；
- FFmpeg filtergraph；
- 纯二维轨道列表；
- Blender scene；
- Remotion React tree；
- 任意外部系统的完整 DSL。

它表达的是 Raccord 自己负责的后期语义：

```text
Project
└── Composition
    ├── Sequence
    │   ├── Clip
    │   ├── Gap
    │   └── Transition
    ├── Stack / Nested Composition
    ├── Caption Track
    ├── Audio Graph
    ├── Color Operations
    └── External Media Action Reference
```

### 4.1 核心对象

#### Project

```text
project_id
revision
metadata
root_composition
asset_catalog
render_lock
```

#### Composition

组合多个媒体源、序列、图层和效果，形成一个可渲染的时间域。

#### Clip

Clip 分开保存两个时间范围：

```text
timeline_range：出现在项目时间线中的范围
source_range：使用源素材中的范围
```

这样可以独立表达：

- trim；
- offset；
- speed；
- freeze frame；
- retime；
- nested composition。

#### SourceRef

SourceRef 只描述媒体来源，不描述其生产技术：

```json
{
  "id": "asset-title-123",
  "kind": "video",
  "uri": "cas://sha256/abc123",
  "duration": "3s",
  "timebase": "30/1",
  "dimensions": [1920, 1080],
  "pixelFormat": "rgba",
  "alpha": true,
  "colorSpace": "srgb"
}
```

如果源是动态外部动作，则使用通用引用：

```json
{
  "kind": "external_media_action",
  "integration": "com.example.remotion",
  "artifact": "cas://sha256/project123",
  "entry": "LowerThird",
  "parameters": {
    "name": "Ada",
    "title": "CFO"
  }
}
```

Raccord 只理解：

```text
这是一个外部媒体动作
它有输入、参数、时间范围和输出媒体
```

Raccord 不理解：

```text
它内部是不是 HTML、React、Blender 节点或 AE expression
```

### 4.2 时间模型

视频时间使用精确的 rational frame/time：

```text
frame rate = 24000/1001
```

音频时间使用：

```text
integer sample index
```

禁止在核心计算中使用浮点秒作为唯一时间表示。

Agent 只使用：

```text
after(clip_id)
before(marker_id)
inside(caption_id)
fill_between(anchor_a, anchor_b)
```

Raccord 内部再解析为 frame 和 sample。

---

## 5. 外部系统接入：Media Action Protocol

当外部系统超过几十个，并且 DSL、运行时、分片能力都不一样时，不能定义一个要求所有系统实现的万能 Plugin IR。

Raccord 采用一个小型的统一执行协议：

```text
Raccord Media Action Protocol（RMAP）
```

RMAP 统一的不是外部系统的创作语义，而是：

```text
输入
动作
参数
能力
时间范围
执行计划
输出 artifact
诊断
```

### 5.1 RMAP 生命周期

```text
Handshake
    ↓
Describe
    ↓
Prepare
    ↓
Inspect
    ↓
Execute
    ↓
Validate / Commit
```

#### Handshake

协商协议版本和能力版本。

#### Describe

返回静态信息：

```text
integration id
protocol versions
supported platforms
runtime requirements
permission requests
output media types
```

#### Prepare

将外部项目打包成不可变的 native bundle：

```text
Remotion source + package lock
.blend + linked assets
HTML project + dependencies
.aep + fonts + plugins
```

这个过程可以编译、打包、解析依赖或检查项目。

#### Inspect

针对具体项目和参数返回：

```text
duration
resolution
fps
audio layout
entry points
dependencies
range rendering plan
resource estimate
```

#### Execute

执行一个具体动作或 shard，返回媒体 artifact。

#### Cancel

由 Host 统一取消，并确保进程树、容器或远程任务最终停止。

### 5.2 Integration Capsule

每个外部系统提供一个独立的 Integration Capsule：

```text
raccord-integration-remotion
raccord-integration-blender
raccord-integration-hyperframes
raccord-integration-aftereffects
raccord-integration-lottie
```

Capsule 内部拥有自己的：

- DSL 解析；
- native compiler；
- runtime 启动方式；
- 依赖发现；
- 参数转换；
- 分片规则；
- 原生错误转换；
- 输出验证。

Raccord Core 不需要为 100 个系统写 100 套内部适配逻辑。

### 5.3 三个例子

#### Remotion

```text
输入：React/TypeScript project
Prepare：bundle + lock dependencies
Inspect：解析 composition、duration、fps、props
Execute：调用 renderMedia/renderFrames
输出：帧序列、视频或中间 artifact
```

Raccord 不理解 React 组件树。

#### Blender

```text
输入：.blend + linked assets
Prepare：收集依赖，打包 scene
Inspect：读取 scene/camera/frame range/render engine
Execute：background render
输出：EXR/PNG sequence 或视频
```

未 bake 的模拟、未知 add-on 或状态依赖可以声明为 sequential/full-only。

#### Hyperframes

```text
输入：HTML/JS/GSAP composition
Prepare：锁定依赖、字体、浏览器和资源
Inspect：声明时长、输出、可 seek 性和资源消耗
Execute：渲染指定范围或完整素材
输出：透明视频、帧序列或其他媒体 artifact
```

Raccord 不需要在 IR 中定义 `html_surface`，也不需要知道 GSAP。

---

## 6. 外部动作的分片和局部渲染

不能使用简单的：

```text
supportsRangeRender: true
```

每个 Capsule 必须对具体项目返回执行计划：

```json
{
  "partition": "independent_frames",
  "preRoll": "0s",
  "postRoll": "0.2s",
  "maxParallelism": 8,
  "audio": "none"
}
```

或者：

```json
{
  "partition": "sequential",
  "preRoll": "2s",
  "postRoll": "1s",
  "maxParallelism": 1
}
```

支持的分片模式：

```text
INDEPENDENT_FRAMES
CONTIGUOUS_CHUNKS
SEQUENTIAL
FULL_ONLY
```

### 6.1 常见情况

```text
Remotion 纯 frame function       → independent frames
Blender 静态场景                 → independent frames
Blender 未 bake 粒子模拟          → sequential/full-only
AE 未知第三方 effect              → conservative sequential
Hyperframes 可确定性 seek         → range render
状态依赖的浏览器动画               → replay 或 bake
```

Raccord Scheduler 不猜测这些规则，只使用 Capsule 的 Inspect 结果。

### 6.2 局部修改

如果修改的是 Raccord 自己的属性：

```text
修改 transform
→ 只重新合成
```

如果修改的是外部动作参数：

```text
修改 Remotion title
→ 外部 action 参数 digest 改变
→ 只失效该 action 的时间范围
```

如果外部源不支持 range rendering：

```text
重新生成该外部 source
或者先 bake 成普通媒体
```

Raccord 不为了优化而假设外部系统具备它没有的能力。

---

## 7. Artifact、缓存和可复现性

所有大数据使用 content-addressed storage：

```text
source asset
native bundle
frame sequence
audio block
render shard
final media
logs
provenance
```

### 7.1 缓存层级

```text
Prepare cache
  source snapshot → native bundle

Inspect cache
  bundle + params + runtime → execution plan

Render cache
  exact action shard → media artifacts

Assemble cache
  ordered artifacts + encoder config → final output
```

### 7.2 Cache key

必须包含：

```text
canonical action
input artifact digests
native bundle digest
parameters
requested range
integration capsule digest
runtime identity
fonts/plugins
codec/color config
hardware class
permission policy
```

不能只使用：

```text
project hash + parameters
```

否则同一个项目换了 Blender、字体、Chromium、GPU driver 或插件版本后，会错误复用旧缓存。

### 7.3 可复现等级

每个外部动作声明：

```text
HERMETIC
SEEDED
ENVIRONMENT_PINNED
BEST_EFFORT
NONDETERMINISTIC
```

只有前几类允许进入普通内容缓存。`BEST_EFFORT` 和 `NONDETERMINISTIC` 必须明确标记，不能伪装成确定性结果。

---

## 8. Agent API：最少 Token，最大精度

Agent API 的设计目标不是让 Agent 看到更多信息，而是让它永远不需要看到不该处理的信息。

### 8.1 Agent 永远不接收

```text
视频帧
音频 sample
完整 waveform
完整 JSON IR
完整媒体库
所有 keyframes
FFmpeg filtergraph
Blender scene 全文
Remotion source 全文
Lambda 日志
内部缓存结构
```

这些信息由服务器处理，Agent 只获得摘要、ID、range handle 和诊断。

### 8.2 核心工具

核心工具控制在少量范围：

```text
find
inspect
plan_edit
commit_edit
verify
render
```

工具 schema 简短，详细能力按需加载。

### 8.3 读取工具

#### `find`

按语义搜索素材、clip、marker、caption：

```json
{
  "query": "第二个采访镜头",
  "kind": "clip",
  "limit": 5
}
```

只返回：

```json
{
  "id": "clip_03",
  "label": "interview take 2",
  "track": "video.main",
  "range": "marker:hook..marker:answer"
}
```

#### `inspect`

只取指定 ID 的细节：

```json
{
  "ids": ["clip_03", "audio_music", "caption_17"],
  "fields": ["timing", "links", "effects"]
}
```

### 8.4 修改工具

Agent 使用稳定 ID 和语义锚点：

```json
{
  "baseVersion": 41,
  "ops": [
    {
      "op": "replace",
      "clip": "clip_03",
      "source": "range_take_2"
    },
    {
      "op": "add_transition",
      "between": ["clip_02", "clip_03"],
      "kind": "crossfade",
      "duration": "0.5s"
    }
  ]
}
```

禁止 Agent 使用：

```text
数组下标
裸 frame number
裸 sample number
数字轨道位置
隐式对象路径
```

### 8.5 Plan → Commit → Verify

```text
plan_edit
    ↓
结构和约束验证
    ↓
preview diff
    ↓
commit_edit（CAS revision）
    ↓
verify
```

`plan_edit` 可以返回：

```json
{
  "feasible": false,
  "errors": [
    {
      "code": "INSUFFICIENT_SOURCE_HANDLE",
      "message": "transition requires 8 frames of source head",
      "suggestion": "use a wider source range"
    }
  ]
}
```

Agent 不需要自己换算 0.5 秒对应多少帧，也不需要自己判断 transition 是否够长。

### 8.6 事务和版本

每个编辑事务包含：

```text
baseVersion
idempotencyKey
semantic operations
```

版本过期时：

```text
STALE_VERSION
```

Raccord 返回当前 projection，Agent 重新规划，而不是继续在旧状态上修改。

### 8.7 外部能力按需加载

Agent 默认不加载 100 个 integration 的 schema。

只有用户明确说：

```text
用 Remotion 做动态标题
```

才加载 Remotion authoring capability 的最小工具描述。

只有用户明确说：

```text
在 Blender 场景中修改相机
```

才加载 Blender capability。

主 Agent 仍然只向 Raccord 提交：

```text
一个外部 action 引用或 artifact 引用
```

而不是把 Blender/Remotion/Hyperframes 的完整 DSL 带进主上下文。

---

## 9. 验证模型

Raccord 不让 Agent 自己判断成功。

验证分为三层：

### 9.1 IR 验证

```text
所有 ID 存在
所有 anchor 可解析
没有非法时间范围
没有孤立 transition
A/V link 合法
媒体源存在
```

### 9.2 Execution 验证

```text
输出时间范围正确
输出帧数正确
音频 sample coverage 正确
尺寸、fps、color metadata 正确
所有必需 artifact 存在
```

### 9.3 结果验证

```text
没有空输出
没有缺帧
没有越界文件
输出 digest 正确
预期的 clip/source/track 状态正确
```

审美判断可以由另一个模型辅助，但不能作为核心 commit gate。核心 gate 必须是确定性检查。

---

## 10. 权限和隔离

外部系统的 native project、脚本、expression、add-on 都应视为可执行内容，而不是普通数据。

默认权限：

```text
CAS 输入只读
scratch/output 独立目录
禁止 home 目录
禁止 credential store
禁止网络
禁止 clipboard/UI automation
限制 CPU/RAM/GPU/disk/time
```

执行方式按风险选择：

```text
WASM       纯解析器、轻量转换
OCI        普通 Linux headless runtime
VM         高风险 native/GPU runtime
Host Broker 需要授权的 macOS/Windows 软件
```

项目 IR 不能自己指定要运行哪个任意镜像。外部 integration 必须由运营方安装、审核和启用。

---

## 11. 服务端和部署

Raccord Core 不应该知道 Lambda、ECS 或 Kubernetes 的细节。

它只产生：

```text
Render Action
```

Scheduler 根据 action 的要求寻找合适的执行环境：

```text
native-cpu-worker
native-gpu-worker
remotion-worker
blender-worker
after-effects-host
```

Lambda 可以是某种 Execution Provider，但不是项目语义。

对于浏览器、Blender、After Effects 这类重型或有状态 runtime，优先使用：

```text
persistent worker pool
ECS / Batch / VM / host broker
```

而不是强行使用 Lambda。

---

## 12. Rust Workspace 结构

Raccord 采用统一的 crate 命名空间：

```text
raccord-<component>
```

这不是 Rust 强制要求，但对于公开 workspace 很有价值：

- 在 crates.io、GitHub 和文档中容易发现；
- 明确 crate 属于 Raccord 项目；
- 避免 `core`、`runtime`、`audio` 等通用名称冲突；
- 允许未来把稳定组件独立发布；
- 让外部 integration 使用同一命名体系。

按职责拆分 crate 本身是合理的，但依赖方向必须清楚，不能只按目录好看地拆分。推荐的 workspace：

```text
raccord/
├── Cargo.toml                 # workspace
├── crates/
│   ├── raccord-time/          # RationalTime、FrameTime、SampleTime、Range
│   ├── raccord-ir/            # Project、Composition、Clip、SourceRef、Commands
│   ├── raccord-timeline/      # timeline resolve、anchors、ripple、placement
│   ├── raccord-constraints/   # overlap、link、transition、duration 等约束
│   ├── raccord-media/         # MediaSource、Frame、AudioBlock、Artifact、backend traits
│   ├── raccord-audio/         # sample-clocked audio graph、bus、mix、ducking
│   ├── raccord-compositor/    # video composition、transform、mask、blend、color
│   ├── raccord-cache/         # cache key、CAS、invalidation、artifact store contract
│   ├── raccord-planner/       # execution DAG、range planning、dependency planning
│   ├── raccord-rmap/          # RMAP schema、protobuf、stdio/gRPC protocol types
│   ├── raccord-runtime/       # worker、execution lifecycle、scheduler、cancel/retry
│   ├── raccord-server/        # server binary、API、project store、job entrypoint
│   └── raccord-cli/           # CLI binary
├── adapters/
│   └── ffmpeg/                # 初期可作为 workspace crate 或 runtime adapter
├── schemas/
│   ├── project.schema.json
│   ├── action.schema.json
│   ├── artifact.schema.json
│   └── capability.schema.json
├── docs/
├── examples/
└── tests/
```

推荐的依赖方向：

```text
raccord-time
    ↓
raccord-ir
    ├── raccord-timeline
    ├── raccord-media
    └── raccord-constraints
             ↓
    raccord-audio / raccord-compositor
             ↓
    raccord-planner
             ↓
    raccord-cache / raccord-rmap
             ↓
    raccord-runtime
       ├── raccord-server
       └── raccord-cli
```

具体依赖仍需遵守：

- `raccord-time` 不依赖业务 crate；
- `raccord-ir` 不依赖 FFmpeg、GPU、网络或 server；
- `raccord-media` 只定义媒体和 artifact 抽象，不实现具体外部系统；
- `raccord-planner` 不启动 worker；
- `raccord-runtime` 负责执行和生命周期，但不重新定义 IR；
- `raccord-server`、`raccord-cli` 是应用入口，不被底层 crate 依赖；
- `raccord-rmap` 只包含协议类型和编解码，不包含 Hyperframes、Blender 或 Remotion 实现。

### 关于拆分粒度

这里的拆分不是因为 Rust 要求“一种能力一个 crate”，而是因为这些边界在 Raccord 中有不同的依赖和演化方向：

- `raccord-time`、`raccord-ir` 是轻量、稳定、可能被外部工具复用的基础 crate；
- `raccord-audio`、`raccord-compositor` 是不同的媒体计算域；
- `raccord-planner`、`raccord-runtime` 分别对应纯计划和实际执行；
- `raccord-server`、`raccord-cli` 是不同的 binary；
- `raccord-rmap` 可能被外部 integration 使用；
- `raccord-media` 是 Core 与具体 FFmpeg/GPU/provider 之间的边界。

如果早期某两个 crate 的边界频繁变化，可以暂时合并为 module，但公开命名仍遵循 `raccord-*`。crate 只有在边界稳定、依赖隔离、需要独立测试或发布时才应进入 crates 目录。

外部 integration 永远单独维护：

```text
raccord-integration-remotion
raccord-integration-blender
raccord-integration-hyperframes
raccord-integration-aftereffects
```

Raccord Core 不依赖这些仓库。

---

## 13. V1 范围

### V1 必须包含

```text
1. 自有 Project IR
2. Rational time 和 sample time
3. Semantic timeline
4. Stable ID 和 anchor
5. Clip / gap / transition / composition
6. 基础视频合成
7. Audio graph 基础能力
8. Caption track
9. CPU reference renderer
10. FFmpeg media adapter
11. 增量缓存
12. plan/commit/verify Agent API
13. 通用 SourceRef
14. RMAP 的最小协议定义
15. 一个本地 process integration example
```

### V1 不包含

```text
1. 公共插件市场
2. 100 个 integration
3. HTML 专用 IR
4. Hyperframes 写死在 Core
5. Blender/Remotion DSL 进入 Canonical IR
6. 自动把所有外部系统转换成统一 scene graph
7. Lambda 专用项目模型
8. 任意 native plugin 进程内加载
9. 完整 NLE UI
10. 复杂 3D、粒子和模拟
```

V1 的正确验证方式不是“接入 100 个系统”，而是先证明同一协议能够容纳三种明显不同的系统：

```text
Remotion：代码/浏览器
Blender：场景/模拟/native runtime
After Effects 或 Hyperframes：外部应用/浏览器/专有运行时
```

---

## 14. 开发顺序

### Phase 1：核心语义

- Project IR；
- stable IDs；
- anchor；
- timeline commands；
- transaction/version；
- constraint diagnostics。

### Phase 2：Core Renderer

- CPU compositor；
- FFmpeg source；
- audio mix；
- captions；
- MP4 export；
- local range render。

### Phase 3：增量渲染

- dependency graph；
- invalidation；
- CAS；
- frame/audio block cache；
- deterministic render lock。

### Phase 4：RMAP

- protobuf schema；
- stdio process runner；
- prepare/inspect/execute；
- artifact validation；
- structured failures；
- conformance tests。

### Phase 5：三个 Integration

顺序建议：

```text
Remotion 或简单程序化 provider
→ Blender
→ Hyperframes / 浏览器 runtime
```

不是因为某个系统更重要，而是为了依次验证：

```text
代码型 runtime
→ native scene runtime
→ 重型浏览器 runtime
```

---

## 15. 最终架构决策

### 决策一：不建立万能 Plugin IR

100 个系统内部语义不同，Raccord 只定义自己的后期 IR，不定义它们的统一内部语言。

### 决策二：使用 Media Action Protocol

外部系统通过 `prepare / inspect / execute` 接入，保留自己的 DSL。

### 决策三：Raccord 拥有最终 Renderer

外部 integration 只产生媒体 artifact 或外部 action 结果，最终时间线、音频、字幕、合成和编码由 Raccord 完成。

### 决策四：HTML 不是 Raccord 概念

Hyperframes 可以生成媒体或提供一个外部 integration，但 Raccord IR 不出现 HTML、GSAP、Chromium 等概念。

### 决策五：Agent 不学习 100 个 DSL

Agent 使用少量核心语义工具，外部 authoring capability 按需加载，输出 opaque artifact/action reference。

### 决策六：局部渲染由执行计划决定

Raccord 不假设所有系统支持随机访问。每个 integration 必须为具体项目返回分片、预读、状态和音频边界。

### 决策七：部署和项目语义分离

Lambda、ECS、GPU、VM、native host 都是执行基础设施，不进入 Project IR。

---

## 16. 研究依据

本设计综合参考了以下项目和资料：

### Pi coding agent

本地安装包中的主要参考位置：

```text
@earendil-works/pi-agent-core/dist/agent.d.ts
@earendil-works/pi-agent-core/dist/agent-loop.d.ts
@earendil-works/pi-coding-agent/dist/core/agent-session.d.ts
@earendil-works/pi-coding-agent/dist/core/extensions/types.d.ts
@earendil-works/pi-coding-agent/dist/core/extensions/runner.d.ts
@earendil-works/pi-coding-agent/docs/extensions.md
@earendil-works/pi-coding-agent/docs/packages.md
@earendil-works/pi-coding-agent/docs/sdk.md
@earendil-works/pi-coding-agent/docs/rpc.md
```

借鉴的不是 Pi 的具体分层，而是：

- 核心只拥有通用机制；
- 外围能力通过明确接口注入；
- Host 拥有生命周期和状态一致性；
- package、extension、runtime、permission 不是同一个概念。

### 媒体和执行系统

- OpenTimelineIO：语义时间线、RationalTime、source range；
- FFmpeg：编解码、媒体处理和 headless execution；
- GStreamer/GES：pipeline、caps、backpressure 和媒体调度；
- OpenFX：时间依赖、ROI、tile 和 effect render contract；
- LV2：音频端口和状态管理；
- Natron：依赖 hash、tile/frame cache 和 headless render；
- Remotion：frame-driven rendering、bundle/inspect/render；
- Blender：native scene、background render 和模拟边界；
- Adobe After Effects：aerender、host broker 和序列分片限制；
- Bazel Remote Execution：content-addressed action、CAS、execution identity；
- OCI/WASI：artifact distribution 和 capability-based isolation；
- MLIR：不同 dialect 不应被假设为同一语义；
- LLM/Agent 工具研究：projection、stable ID、patch、外部验证和 token-efficient tool use。

---

## 17. 一句话总结

> **Raccord 自己拥有后期语义和最终渲染；外部系统保留自己的 DSL，通过统一的 Media Action Protocol 提供媒体动作；Agent 只使用稳定、少量、语义化的命令，并由 Raccord 负责时间计算、约束验证、局部渲染和最终交付。**
