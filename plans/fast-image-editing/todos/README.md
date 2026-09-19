# fast-image-editing 执行队列

依据：../plan.md，方案提交 eb2e635。仓库尚无产品源码；下列路径为按方案模块划分的待建路径，01 建立真实布局后，后续任务按实际模块位置实现，不另造平行架构。

## 执行偏好

default_agent: codex
default_model: gpt-6-astra
default_reasoning_effort: max

用户本次要求优先 Codex GPT-6 / max，仅确定性很强的简单任务可用 OpenCode Alibaba DeepSeek Flash 4.1。本队列均为新模块或跨模块验收，没有确定性足够强的简单任务，全部继承 Codex。实际模型名已由本机模型元数据核对为 gpt-6-astra，支持 max。

## 优先级

| 文件 | 优先级 | 难度 | agent | 模型 / 推理强度 | 状态 | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| 01-foundation.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 明确首版语义，建立可编译的 CLI 与处理核心 |
| 02-geometry-codecs.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 几何变换、PNG/JPEG、方向与透明度 |
| 03-adjustments-filters.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 调色、曲线、模糊、锐化与像素验证 |
| 04-project-history.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 自包含素材、不可变 ops、原子提交和跨进程历史 |
| 05-replay-preview.md | P0 | hard | codex（继承） | gpt-6-astra / max | pending | 检查点、缓存失效、坐标预览、修改旧步骤和模板 |
| 06-layers-masks.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 图层、无损变换、混合、蒙版、选区 |
| 07-groups-text-adjustment.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 分组、剪贴蒙版、文字和调整层 |
| 08-smart-backend-evaluation.md | P2 | hard | codex（继承） | gpt-6-astra / max | pending | 真实模型可行性、质量和冷暖调用评估 |
| 09-smart-cutout-erase.md | P2 | hard | codex（继承） | gpt-6-astra / max | pending | 真实抠图、分割、移除修复及结果资产 |
| 10-generative-fill-outpaint.md | P2 | hard | codex（继承） | gpt-6-astra / max | pending | 真实生成填充和扩图 |
| 11-core-performance.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | 普通编辑与工程模式端到端测量和优化 |
| 12-agent-packaging.md | P1 | hard | codex（继承） | gpt-6-astra / max | pending | agent 文档、示例和基础功能打包验收 |
| 13-smart-acceptance.md | P2 | hard | codex（继承） | gpt-6-astra / max | pending | 三类能力串联、智能性能与最终交付验收 |

## 文件

按以下顺序选取当前可运行任务；依赖和实际产品文件重叠优先于并行数量。

1. 01-foundation.md；依赖：无。
2. 02-geometry-codecs.md；依赖 01-foundation.md。
3. 03-adjustments-filters.md；依赖 02-geometry-codecs.md。
4. 04-project-history.md；依赖 03-adjustments-filters.md。
5. 05-replay-preview.md；依赖 04-project-history.md。
6. 06-layers-masks.md；依赖 05-replay-preview.md。
7. 07-groups-text-adjustment.md；依赖 06-layers-masks.md。
8. 08-smart-backend-evaluation.md；依赖 01-foundation.md；仅写独立评估脚本与报告，可与 02–07 并行。
9. 09-smart-cutout-erase.md；依赖 07-groups-text-adjustment.md、08-smart-backend-evaluation.md 选型结果和必要的真实运行环境。
10. 10-generative-fill-outpaint.md；依赖 09-smart-cutout-erase.md、08-smart-backend-evaluation.md 中可用生成后端。
11. 11-core-performance.md；依赖 07-groups-text-adjustment.md；与 09/10 涉及相同核心或 CLI 文件时串行，智能后端阻塞时仍可推进。
12. 12-agent-packaging.md；依赖 11-core-performance.md；与智能实现发生产品文件重叠时串行，普通编辑验收不依赖智能服务可用。
13. 13-smart-acceptance.md；依赖 08–10、12 全部完成，并具备实际模型/服务与样例质量验收条件。

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

除此之外执行各 todo 的专门验收。性能目标须实际测量；真实模型验收不能用 mock 取代。当前机器作为首轮开发/基准环境，跨平台支持范围、模型条件和未确认事项须在文档准确区分；不得把未确认兼容性写成已承诺或已实现。

## 范围外

命名分支及合并、PSD 完整无损往返、RAW、专业印刷色彩、完整笔刷、GUI、自动迁移内容相关蒙版、分块渲染和多完整渲染后端留作 roadmap，不进入本轮。
