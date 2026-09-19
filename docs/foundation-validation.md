# 任务 01 历史验收记录

以下保留任务 01 完成时的历史证据，24 个测试不是当前仓库测试总数。当前直接编辑/打包校验见 [任务 12 验收](packaging-validation.md)，性能见 [当前报告](performance.md)。

2026-09-19，任务 01 worktree，Linux x86_64 / Rust 1.98.1。实现范围为基础 workspace、codec 子集、identity/空管线、结果与后续工程契约。智能能力仅标记 roadmap，未增加任何模型后端或依赖。plan.md 保持原样，由协调器统一更新范围。

## 验收对应关系

| 条目 | 代码与证据 |
| --- | --- |
| 可编译 workspace 与 CLI | `pic-core` + `pic-cli`；debug/release 均通过。CLI 测试 `help_version_and_capabilities_are_truthful_and_json_safe` 实际启动 help、version、capabilities，断言 supported/partial/not_implemented 三态及智能 roadmap |
| 统一操作/管线接口、顺序与路径 | `single_operation_and_pipeline_share_results_and_use_pipeline_resource_base` 比较 identity 命令和一步管线的步骤结果及解码像素。核心 `identity_steps_preserve_float_bits_and_shared_storage` 断言 0/1/2 逻辑顺序、全部 f32 位模式和共享像素存储；`resources_resolve_from_pipeline_directory_not_the_process_directory` 在独立素材目录核对相对/绝对路径。未来修改像素的操作须补充非交换算法顺序测试 |
| JSON、错误与真实 I/O | CLI 测试 helper 对每次 JSON 调用校验单行 stdout、schema/engine version、ok、错误/data 排他关系及全部阶段数值。PNG 空管线、JPEG info/run、JPEG↔PNG、灰度/alpha、内容识别和显式输出格式均真实执行并重新解码验证像素；参数、JSON、未知操作/版本/目标、文件、格式、元数据及资源错误返回非零 |
| 覆盖、原子发布和失败清理 | CLI `overwrite_is_explicit_and_in_place_processing_is_atomic` 验证显式覆盖、原地输入输出、透明图失败保留旧目标。核心 `only_one_concurrent_no_clobber_publication_succeeds` 验证两个并发发布只有一个赢家、内容完整；codec 单元测试 `partial_write_failure_never_publishes_or_replaces_a_target` 在写入部分字节后注入错误，断言新目标不存在、旧目标未改、临时文件已清理。目录/符号链接/非法路径同样不发布输出 |
| 语义、硬件及剩余事项 | [执行契约](foundation-contract.md) 定义线性 RGBA32F、straight alpha、坐标、EV/倍率、资源限额、版本/稳定 ID 与未来检查点精度；[矩阵](capability-matrix.md) 区分功能、参数、质量、文件保真、时延；[基线](baseline.md) 记录机器、30 次启动/小图测量和未知事项，没有 Photoshop 完全兼容或未经测量的性能宣称 |

测试源码：[核心集成](../crates/pic-core/tests/core.rs)、[发布失败注入](../crates/pic-core/src/codec.rs)、[CLI 集成](../crates/pic-cli/tests/cli.rs)。PNG 全部 256 级通道值及 alpha=0 的隐藏颜色严格往返一致；浮点测试包含非 8-bit 量化值、负 RGB、HDR RGB、负零和小数 alpha，证明步骤之间没有量化。JPEG 只对固定小样本断言尺寸与通道误差，不作为通用有损编码误差保证。

## 实际运行命令

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，无 warning；包含基线 example |
| `cargo test --workspace` | 通过，24 个测试：14 个 CLI 集成、9 个核心集成、1 个部分写入失败单元测试 |
| `cargo build --release` | 通过，生成 `target/release/pic-cli` |
| `git diff --check` | 通过；暂存新增文件后另跑 `git diff --cached --check` |
| `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli` | 14 个集成测试全部通过；测试 harness 启动实际 release CLI，使用临时自生成 PNG/JPEG、管线及输出，覆盖 help/version/capabilities/info/run/identity |
| `cargo run --release -p pic-cli --example foundation_baseline -- "$PWD/target/release/pic-cli"` | 通过；5 组各 1 次预热 + 30 次独立进程，断言 JSON 与输出像素，结果记录在 baseline.md |

临时素材与输出均由 tempfile 管理；构建产物仅在忽略的 target/。没有向 Git 添加任何图片或测量产物。

## 交接边界

任务 01 无剩余 blocker。后续操作沿用 `operation::OperationSpec` / `Operation::apply`、`Pipeline::single` / `Pipeline::execute` / `pipeline::run`，采用明确版本与资源解析。`document::Raster` 是工作像素，不是可独立修改的持久化工程状态；源素材/不可变 ops 才是后续工程权威依据。

任务 01 当时仅覆盖基础 codec/identity，并拒绝 EXIF；这是历史范围，不是当前能力。后续任务已实现几何/调色、图层/文字、历史/检查点/预览、EXIF 方向 1–8 归一化及显式 JPEG 铺底，当前范围见 [功能矩阵](capability-matrix.md)。ICC/gamma/chromaticity/HDR、动画与高位深仍拒绝。Linux 为已验证平台；无 fsync 保证、崩溃遗留临时文件自动清理或严格 RSS 上限。大图性能已测，见 [任务 11 报告](performance.md)。模型功能仍未实现。
