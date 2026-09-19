# fast-image-editing 执行队列

依据：../plan.md。用户最新限定本轮只做无需 LLM/模型的直接编辑；原智能任务 08/09/10/13 移到 ../roadmap/。保留已有任务编号以便跟踪。仓库尚无产品源码；下列路径为按方案模块划分的待建路径，01 建立真实布局后，后续任务按实际模块位置实现，不另造平行架构。

## 执行偏好

default_agent: codex
default_model: gpt-6-astra
default_reasoning_effort: max

用户本次要求优先 Codex GPT-6 / max，仅确定性很强的简单任务可用 OpenCode Alibaba DeepSeek Flash 4.1。本队列均为新模块或跨模块验收，没有确定性足够强的简单任务，全部继承 Codex。实际模型名已由本机模型元数据核对为 gpt-6-astra，支持 max。

## 优先级

| 文件 | 优先级 | 难度 | agent | 模型 / 推理强度 | 状态 | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| [done/01-foundation.md](done/01-foundation.md) | P0 | hard | codex（继承） | gpt-6-astra / max | done | 基础 workspace、真实 codec/管线、契约及 24 个测试完成；release 验收通过 |
| 02-geometry-codecs.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 几何变换、PNG/JPEG、方向与透明度 |
| 03-adjustments-filters.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 调色、曲线、模糊、锐化与像素验证 |
| 04-project-history.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 自包含素材、不可变 ops、原子提交和跨进程历史 |
| 05-replay-preview.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 检查点、缓存失效、坐标预览、修改旧步骤和模板 |
| 06-layers-masks.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 图层、无损变换、混合、蒙版、选区 |
| 07-groups-text-adjustment.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 分组、剪贴蒙版、文字和调整层 |
| 11-core-performance.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 普通编辑与工程模式端到端测量和优化 |
| 12-agent-packaging.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | agent 文档、示例和基础功能打包验收 |

## 文件

按以下顺序选取当前可运行任务；依赖和实际产品文件重叠优先于并行数量。

1. [done/01-foundation.md](done/01-foundation.md)；依赖：无；已完成并保留验收证据。
2. 02-geometry-codecs.md；依赖 01-foundation.md。
3. 03-adjustments-filters.md；依赖 02-geometry-codecs.md。
4. 04-project-history.md；依赖 03-adjustments-filters.md。
5. 05-replay-preview.md；依赖 04-project-history.md。
6. 06-layers-masks.md；依赖 05-replay-preview.md。
7. 07-groups-text-adjustment.md；依赖 06-layers-masks.md。
8. 11-core-performance.md；依赖 07-groups-text-adjustment.md。
9. 12-agent-packaging.md；依赖 11-core-performance.md。

直接编辑各阶段均扩展同一操作/CLI/工程契约，按此依赖链串行实施，不为了并行而重叠写公共模块。智能选型任务已不在本轮，不再作为可并行工作分发。

共享的 todo README 状态和独立 todo 归档属于队列记录，集成时串行保留全部状态；产品文件有重叠的任务不并发写。每项一个 worktree、一个最终任务 commit，完成全部验收后才移至 done/，blocked 不归档。

## 仓库校验

01 建立 Rust workspace 后，每项集成必须在任务 worktree 运行：

```sh
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

除此之外执行各 todo 的专门验收。性能目标须实际测量。当前机器作为首轮开发/基准环境，跨平台支持范围和未确认事项须在文档准确区分；不得把未确认兼容性写成已承诺或已实现。

## 范围外

智能抠图/分割/修复、生成填充/扩图及其模型选型和验收均按用户指令延期，详见 ../roadmap/README.md。命名分支及合并、PSD 完整无损往返、RAW、专业印刷色彩、完整笔刷、GUI、自动迁移内容相关蒙版、分块渲染和多完整渲染后端也不进入本轮。
