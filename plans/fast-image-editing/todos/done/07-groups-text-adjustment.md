difficulty: hard
agent: inherit

# 分组、剪贴蒙版、文字和调整层

## T1 · 完成基础图层编辑闭环

前置依赖：06-layers-masks.md。

要做什么：实现图层组和明确的组渲染语义、剪贴蒙版依赖、非破坏调整层，循环引用/非法层级检测。实现显式字体绑定的文字图层，具备真实排版处理，明确 Unicode/中文/缺字/换行/对齐支持范围；嵌入或明确绑定可分发字体资源，缺失时不能静默替换。所有功能以已有 ops 提交、资产、历史、检查点与预览扩展，不能引入平行状态源。

预计修改：pic-core document/composite/text/operation，pic-cli 相应入口，合法字体测试素材/说明、docs 与测试，本 todo/README。

验收条件：

- [x] 分组显隐/顺序/变换与剪贴蒙版覆盖、调整层作用范围有解析样例/渲染测试；非法层级和依赖环被拒绝。
- [x] 文字有明确字体文件、文字参数和可重放排版；中文或受支持 Unicode 样例、换行/对齐有实际渲染证据，缺字/字体缺失准确报告。
- [x] 保存工程再打开修改、撤销/重做、清缓存重放和多图层检查点保留分组、字体、剪贴与调整参数。
- [x] 背景+主体+现成蒙版+文字→保存→重开修改→导出里程碑真实执行，检查像素、尺寸、透明度、位置、历史。
- [x] 完整仓库校验通过，帮助与能力矩阵准确。

验证：完整仓库校验；图层里程碑、字体资产移动与丢失、层级/剪贴环、前后预览与全尺寸导出比对。


## 完成与验收记录（2026-09-19）

实现记录：新增 group_add、layer_parent、layer_clip、text_add、text_set、adjustment_add、adjustment_set 七个 v1 ops，共 30 个公开操作。它们沿用同一 Document、内容寻址资产、不可变历史、expected_revision、预览和导出；PICDOC03 与渲染身份保留全部新增参数。仅新增排版所需 rustybuzz=0.20.1、ab_glyph=0.2.32、unicode-script=0.5.8 及传递依赖，未升级原依赖。字体与许可证在 tests/fonts/。

| 验收条件 | 具体证据 |
| --- | --- |
| 分组/剪贴/调整及依赖验证 | `crates/pic-core/tests/groups_text.rs`：隔离组边界/显隐/顺序/变换/opacity，解析 clip 链与组 alpha，五种调整同原点操作，跨作用域影响拒绝；环、非法 parent/before/clip、引用目标删除准确报错。CLI 单命令历史与一次性管线输出相同 |
| 真实文字 | 测试 DejaVu Sans 字体明确绑定；ffi 形成单字形、AV 有真实字距、组合 é 与预组合像素相同；Café/Ωμέγα/Привет 实际渲染；LF、居中/右对齐和溢出裁切；缺字 U+4E2D、缺文件及不支持脚本/控制字符报错 |
| 工程与完整检查点 | `project/tests/groups_text.rs`：逐层/蒙版 f32 位与 DocumentInfo 比较，字体内嵌、原导入文件删除与工程移动后续编 text/adjustment/group，undo/redo、PICDOC03、清缓存重放一致；旧 PICDOC02 回退；字体丢失/损坏不能被预览/检查点命中掩盖 |
| 完整里程碑 | `crates/pic-cli/examples/layer_milestone.rs` 为可复用入口，集成测试复用实际新进程流程；192×128，r9→保存/移动→r10 文字修改，2 commits / 10 steps；主体 mask 零/半/满像素分别 `[0,0,0,0]` / `[255,0,0,128]` / `[255,0,0,255]`，文字覆盖 1446 像素；前后 direct/preview/export 相同，undo/redo 与冷重放相同 |
| 全仓库/帮助/矩阵 | 下列全部命令退出 0；30 个操作及新增帮助准确，文字和调整支持范围明确；无目标/字体重绑定规则的模板准确拒绝 |

协调器指出的边界已修复：按作用域实际同时存活的 back/front/output、adjusted 面、子组面和 clip alpha 分别准入，原始图层只计一次。8×4 双层在 2560 字节通过、2559 拒绝，clip 额外 128 字节及组内 adjustment 峰值分别验证。3840×2160 双层准入 663552000 字节，默认 1 GiB 下执行成功。27 层 1e6 缩放行列式溢出与 `Affine([1e160,0,1e160,1e-160,0,0])` 的 NaN 往返均拒绝；失败 manifest/revision 不变，正常 27 层映射往返通过。legacy fast path 拒绝非 root raster、新 kind 和 clip。

验证命令与结果：

```sh
cargo fmt --all -- --check                           # PASS
cargo clippy --workspace --all-targets -- -D warnings # PASS
cargo test --workspace                              # PASS，127 项
cargo build --release                               # PASS
git diff --check                                    # PASS
PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli --test project
# PASS，25 CLI + 24 project = 49 项，实际调用 release 二进制
cargo run --release -p pic-cli --example layer_milestone -- "$PWD/target/release/pic-cli" /tmp/pic-layer-milestone-07-final --4k
# PASS，普通 4K 双图层一次性与工程导出像素一致，corner RGBA [188,0,187,255]
```

最终 release 可复用产物：`/tmp/pic-layer-milestone-07-final/report.json`、`evidence.json`、前后 direct/preview PNG、`relocated/final.png`、`relocated/work.pic`、4K 输出和工程。完整步骤脚本存于仓库，临时素材和生成输出不提交。

本机一次普通 4K run 资源测量为 2.13 秒、峰值 RSS 686144 KiB（`/tmp/pic-layer-milestone-07/4k-run-time.txt`），这是单次正确性/资源检查，不是性能基准。协调器另行独立验证六组 release 场景、旧 task06 工程，以及整张 4K 双层+opacity 的解析像素 `[152,103,153,178]`；其记录 `/tmp/pic-fast-image-editing-20260919/07-4k-probe/result.json` 报告峰值 686312 KiB。准入限额不宣称进程 RSS 保证。

文档：[语义、参数及证据](../../../../docs/groups-text-adjustments.md)、[能力矩阵](../../../../docs/capability-matrix.md)、[字体来源和许可](../../../../tests/fonts/README.md)。明确范围：水平 LTR Latin/Greek/Cyrillic，每行一个字母表加 Common/Inherited；无 CJK/bidi/自动换行/字体回退；隔离固定边界组，无 pass-through；调整层仅点调整。没有外部 blocker，LLM/模型仍仅 roadmap。集成阶段已执行 `git rebase main`，基准 main 为 `0a5591b86d235de5c80fc451d4b81d15fcaad666`；分支已基于该最新提交，无冲突。plan.md 与其他任务状态保持原样，仅 amend 当前任务 commit，未进行 merge/push/PR。

集成阶段复验：fmt、clippy（-D warnings）、workspace 127 项测试、release build、diff-check 全部通过；release CLI 49 项测试以及 layer_milestone --4k 再次通过。此次产物为 `/tmp/pic-07-integration-b77Obd/artifacts/report.json`，无需产品代码修复，保留原有全部验收证据。
