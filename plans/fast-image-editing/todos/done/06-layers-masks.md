difficulty: hard
agent: inherit

# 图层、合成、选区与蒙版

## T1 · 扩展现有 ops 工程的可编辑图层

前置依赖：05-replay-preview.md。

要做什么：扩展既有 document/ops 到稳定 ID 图层、有序绘制、显隐、不透明度、无损位置/缩放/旋转/翻转，normal/multiply/screen/overlay；外部图像蒙版、矩形/区域选区与坐标空间规范。单次 composite 与工程渲染共用核心。持久化各独立图层、蒙版、参数与变换，检查点不能仅保存扁平图。增加单图层/蒙版预览和画布↔图层坐标映射；已有单图工程兼容策略明确。

预计修改：pic-core composite/document/operation/pipeline，pic-cli 的图层/蒙版/选区入口，docs/测试，本 todo/README。

验收条件：

- [x] 合成顺序、四种混合模式和 opacity/alpha 使用已知像素解析期望验证，变换和蒙版不会产生错误黑边。
- [x] 图层 ID 稳定，编辑/重排/移除/选择目标不会依赖名字或索引；不合法目标/尺寸/蒙版有结构化错误。
- [x] 图层、选区和蒙版可跨进程保存、撤销/重做/重放/检查点恢复，各图层仍独立可编辑。
- [x] 全图、指定图层、蒙版预览与导出共享渲染语义，变换后的区域坐标映射有回归测试。
- [x] 原有 P0 流程保留，全部仓库校验通过。

验证：完整仓库校验；多层小图解析像素、变换/裁剪/蒙版、移动工程、检查点恢复后继续编辑的集成测试。


## 完成记录（2026-09-19）

实现位于现有统一核心，没有新增平行权威状态。原资产 + 不可变 ops 仍是恢复依据；Document 保存独立图层、蒙版、参数、变换、选区与已使用 ID，检查点仅为可重建副本。无模型／生成工具调用，无依赖新增或升级。完整参数、兼容策略及范围见 [图层与蒙版契约](../../../../docs/layers-masks.md)。

### 逐条验收证据

1. **合成与像素**：`crates/pic-core/tests/layers.rs` 的 `all_blend_modes_match_analytic_overlap_opacity_and_alpha` 解析验证 normal/multiply/screen/overlay、双半透明 alpha 与 opacity；另覆盖顺序、显隐、0 alpha、128/255 coverage、coverage×alpha、平移与 37° 旋转的透明／被遮蔽颜色边缘。CLI 四种模式的 composite、JSON 管线与工程导出解码像素一致。
2. **稳定目标与错误**：同名层重排、重命名、隐藏、移除均按稳定 ID；删除的 ID 在该祖先路径不可复用。非法／自身参照、非法 target／尺寸／变换、彩色／错尺寸蒙版和无效选区返回结构化错误，manifest 不变。多层 canvas 仅支持 identity、无损 crop、透明 canvas；歧义操作显式拒绝。
3. **持久化与历史**：`layers_masks_selection_checkpoint_move_undo_redo_and_continue_across_processes` 在独立 CLI 进程保存多层检查点、移动工程、删除外部图像和蒙版，再编辑／undo／redo／清缓存重放及继续独立裁剪。核心测试逐 f32 位比较各层／蒙版，覆盖 HDR、负值、隐藏色、signed zero、subnormal、完整状态恢复、损坏回退、有效摘要的扁平快照拒绝和资产／ops／manifest 写失败。缓存命中仍检查全部前缀必需资产。
4. **预览及坐标**：canvas、指定层、mask:<ID> 预览与导出共享 Document 渲染器；区域旋转预览返回双向 layer/canvas/preview 仿射矩阵，并验证缓存命中坐标不变；画布／本地空间矩形并集对变换后的像素中心选区正确。`reduced_mask_preview_resamples_coverage_before_display_gamma` 验证 0／255 coverage 缩小后为 128，与图层 alpha 一致，而非 gamma 后约 188。
5. **P0 与完整校验**：旧 schema-v1 单 canvas 工程无需迁移，原像素、组历史、中间步骤、撤销／重做、并发及缓存预算测试保留。workspace 共 109 项测试通过；release CLI／project 共 46 项通过。

### 协调器审查项

- `bind_pipeline` 保留原 Pipeline 与 Project 的较紧 max_dimension/max_pixels/max_buffer_bytes/max_input_bytes，并把有效限制传入外部和内嵌图层／蒙版素材读取及解码。聚焦测试 `project_binding_preserves_tighter_pipeline_admission_and_asset_limits` 用 8×6 工程 + max_dimension=4 的 identity、像素／缓冲限制、图层与 mask 编码字节限制及 asset 引用验证拒绝并保持 manifest。
- Project::restore 完整渲染、指定图层／蒙版预览渲染及 coverage 可视化均计入 process_ms。初次导入素材读取计入 read_ms，保存计入 write_ms；操作内的已绑定素材读取／解码随执行计入 process_ms，文档与代码一致。
- 新图层／蒙版／选区／composite 和非 canvas 像素目标模板导出、以及手写模板导入明确拒绝；不沿用旧素材、蒙版和绑定。
- `snapshot::decode` 对初始层加历史操作数使用饱和加法，同时约束层数及已用 ID 数；`layered_checkpoints_restore_with_default_and_maximum_history_limits` 在默认限额及 `usize::MAX` 下验证磁盘检查点命中、零重放、完整图层／已删除 ID 保留与逐 f32 位一致。先复现 debug 溢出，修复后 debug、release 聚焦回归均通过。
- 基础契约已准确描述 `Document::apply` 分派、主输入单次解码及外部图层／蒙版素材分别解码，并标明已实现 opacity 的 alpha 合成语义。
- 集成阶段执行 `git rebase main`；main 为 `090d0ed`，当前分支已基于该提交，无冲突。保留完整队列和其他任务状态，只更新本任务；智能功能仍在 roadmap。

### 验证命令与结果

```text
cargo fmt --all -- --check                                   PASS
cargo clippy --workspace --all-targets -- -D warnings         PASS
cargo test --workspace                                      PASS (109 tests)
cargo build --release                                       PASS
git diff --check                                            PASS
PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli --test project
                                                            PASS (46 tests)
cargo test -p pic-core --lib layered_checkpoints_restore_with_default_and_maximum_history_limits
                                                            PASS (1 test)
cargo test -p pic-core --release --lib layered_checkpoints_restore_with_default_and_maximum_history_limits
                                                            PASS (1 test)
```

### 已明确边界

无外部 blocker。选区为矩形并集，不做智能选区。带蒙版或选区的破坏性几何需先解除约束；无损 layer_transform 可直接使用。图层 bilinear 缩小时使用四点采样，不承诺抗混叠缩小或 Photoshop/PSD 像素兼容。多层模板重绑定暂不支持。分组、文字、调整层和性能优化属于后续 todo，未实施。Linux/POSIX、无 fsync 保证沿用原有边界。

### 修改文件

当前 todo 从 `todos/06-layers-masks.md` 移至 `todos/done/06-layers-masks.md`；其余修改／新增文件如下。plan.md 和其他 todo 状态未修改。

- `README.md`
- `crates/pic-cli/src/layers.rs`
- `crates/pic-cli/src/main.rs`
- `crates/pic-cli/src/project.rs`
- `crates/pic-cli/tests/cli.rs`
- `crates/pic-cli/tests/project.rs`
- `crates/pic-cli/tests/project/layers.rs`
- `crates/pic-core/src/capabilities.rs`
- `crates/pic-core/src/codec.rs`
- `crates/pic-core/src/composite.rs`
- `crates/pic-core/src/document.rs`
- `crates/pic-core/src/error.rs`
- `crates/pic-core/src/lib.rs`
- `crates/pic-core/src/operation.rs`
- `crates/pic-core/src/operation/layers.rs`
- `crates/pic-core/src/pipeline.rs`
- `crates/pic-core/src/project.rs`
- `crates/pic-core/src/project/assets.rs`
- `crates/pic-core/src/project/cache.rs`
- `crates/pic-core/src/project/history.rs`
- `crates/pic-core/src/project/preview.rs`
- `crates/pic-core/src/project/snapshot.rs`
- `crates/pic-core/src/project/template.rs`
- `crates/pic-core/src/project/tests.rs`
- `crates/pic-core/src/project/tests/layers.rs`
- `crates/pic-core/src/project/tests/replay.rs`
- `crates/pic-core/tests/core.rs`
- `crates/pic-core/tests/layers.rs`
- `docs/capability-matrix.md`
- `docs/foundation-contract.md`
- `docs/layers-masks.md`
- `docs/project-history.md`
- `docs/replay-preview.md`
- `plans/fast-image-editing/todos/README.md`
- `plans/fast-image-editing/todos/done/06-layers-masks.md`
