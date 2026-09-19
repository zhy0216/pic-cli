difficulty: hard
agent: inherit

# 调色与滤镜

## T1 · 交付明确数值语义的普通像素编辑

前置依赖：02-geometry-codecs.md。

要做什么：实现曝光 EV、亮度、对比度、饱和度倍率、色阶、曲线、灰度、反相、模糊和锐化；固定范围、默认值、通道、色彩空间、边界采样与 alpha 行为。提供单操作命令和同一操作类型的有序 JSON 管线；未知或非有限参数必须拒绝。以标量明确语义为准，融合/线程优化留给测量任务，不改变逻辑步骤。

预计修改：pic-core operation 中调色/滤镜与 pipeline 的相应接入，pic-cli，操作文档和测试，本 todo/README。

验收条件：

- [x] 所有列出的操作可真实执行；CLI 与 JSON 参数语义一致，能力查询和帮助准确。
- [x] 恒等参数、小型颜色块、曲线控制点与色阶边界、透明像素、模糊边缘和锐化输出均有独立预期的像素测试。
- [x] 不同操作顺序可产生应有差异，多步只解码一次且最终编码一次（非显式观察/持久化）。
- [x] 不支持/非法参数错误不产生成功输出；alpha 和内部精度保持与核心契约一致。
- [x] 全部仓库校验通过。

验证：完整仓库校验；真实 CLI 的几何+调色+滤镜多步导出及单操作/管线结果对照。

## 完成记录

2026-09-19，独立任务分支 `herdr/plan-fast-image-editing-03-adjustments-filters` 完成，等待协调器集成。

全部功能扩展既有 `OperationSpec/Operation/Pipeline/Raster`，没有增加平行状态源、模型调用或依赖变更。`adjust` 提供曝光 EV、亮度、对比度、饱和度，另有 levels、curves、grayscale、invert、blur、sharpen；单命令与 JSON 同核心。参数、默认值、颜色/alpha、边界与公式见 [调色与滤镜契约](../../../../docs/adjustments-filters.md)。工程权威状态、检查点精度及 expected_revision 的既有契约保持不变。

| 验收条件 | 实际证据 |
| --- | --- |
| 操作真实执行、CLI/JSON 同语义、帮助和能力准确 | 真实 CLI 的 `every_photo_command_matches_its_explicit_json_parameters_and_pixels` 验证 18 组调用、七类操作、四个 adjust 控制与选色通道，比较完整步骤参数和解码 RGBA，并核对 help/capabilities；release 二进制同样通过 |
| 独立像素预期 | 新增 13 个核心测试覆盖位级恒等、原色亮度、各数值控制、色阶黑白/gamma/范围外延、非单调曲线与控制点、单通道、透明隐藏色、已知高斯核的一维/二维边缘、unsharp 负值/高亮过冲；混合 CLI 管线 PNG 独立预期 171/85、alpha 128 |
| 顺序与编解码次数 | 曝光/亮度交换顺序得到不同解析结果；+20/-20 EV 保留 HDR 并恢复原图；`multistep_file_run_decodes_and_encodes_once` 用仅测试启用的 thread-local 计数器实测五步文件处理一次解码、一次编码，并保留全部逻辑步骤 |
| 失败无成功输出、alpha 与精度 | 所有浮点参数的 NaN/±Inf、未知/缺失/位置数组参数、非法范围/通道/控制点均拒绝；缓冲预算及 f32 溢出有测试；CLI 非零退出、data=null、无新文件/旧目标不变、执行溢出不进入编码或发布；未选通道及 alpha 位级验证 |
| 全部仓库校验 | 下表全部通过；全仓库 58 个测试、实际 release CLI 的 25 个集成测试通过 |

| 验证命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，无 warning |
| `cargo test --workspace` | 58 通过：25 CLI + 13 调色/滤镜 + 18 既有核心集成 + 2 codec 单元测试 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过；归档和暂存后再次检查 diff |
| `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli` | 25 通过，包含真实几何+调色+滤镜 PNG/JPEG 导出、单操作与 JSON 对照、错误与原子输出 |

修改文件：

- `crates/pic-core/src/operation.rs`、`operation/adjustments.rs`、`capabilities.rs`、`codec.rs`（仅增加测试计数及测试）、`crates/pic-core/tests/adjustments.rs`。
- `crates/pic-cli/src/main.rs`、`crates/pic-cli/tests/cli.rs`。
- 根 `README.md`、`docs/adjustments-filters.md`、`docs/foundation-contract.md`、`docs/capability-matrix.md`。
- 当前 todo 归档与 `todos/README.md` 的任务 03 状态；保留其他任务状态，未改 `plan.md`。

无外部 blocker。当前仅验证 Linux；高斯使用标量实现，大图/大 sigma 的性能优化留给任务 11，未宣称性能目标达成。不承诺 Photoshop 滑杆/像素兼容或跨 CPU 浮点逐位一致；所有本轮参数语义已明确。未 push、创建 PR、rebase/merge 或操作其他任务分支。
