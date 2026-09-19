difficulty: hard
agent: inherit

# 首版契约和 Rust 基础骨架

## T1 · 建立可编译、可扩展的实际处理入口

前置依赖：无。

要做什么：读取方案和 AGENTS.md，建立 pic-core 库与 pic-cli 二进制的 Cargo workspace。用当前机器作为首轮开发与性能基线，文档化 PNG/JPEG、8-bit sRGB 输入、内部像素精度与 alpha、坐标、资源限制、EV/倍率等参数语义及直接编辑能力对齐矩阵；未实现能力必须如实声明，模型功能仅标未来规划。内部表示须支持后续检查点无损恢复，不能在步骤间无意降低精度。建立 codec、operation、pipeline、document 的清晰边界、统一错误/结果版本和阶段计时。CLI 至少提供 help/version/capabilities/info/run，run 可执行空操作管线完成真实读写，未知操作拒绝。采用 image 起步；依赖选择以本机实际可构建为准，不引入多套完整后端。记录 schema_version/revision/op_version/stable target 的契约，后续实现不在本任务展开。

预计修改：待建 Cargo.toml、Cargo.lock、crates/pic-core/、crates/pic-cli/、README.md、docs/ 下基础设计/功能矩阵；.gitignore 增加构建产物；本 todo 及队列状态。这是仓库首次源码创建，最终布局应在 README 中明确。

验收条件：

- [x] 真实 Rust workspace 编译通过，CLI 的 help/version/info/capabilities/run 可调用；能力查询区分支持、部分支持和未实现。
- [x] 单操作与管线复用核心接口；管线顺序和资源相对路径（管线目录）有明确契约；JSON stdout 仅输出稳定版本结果，日志走 stderr，非法输入为非零退出码。
- [x] PNG/JPEG 最小真实输入输出、空管线、文件/格式/JSON/参数错误及覆盖控制有集成证据；输出以临时文件后原子发布，失败不留下成功目标。
- [x] 文档给出执行语义、基准机器信息和剩余未知事项；未声明 Photoshop 参数/文件完全兼容或未经测量性能。
- [x] 运行队列 README 中全部仓库校验，并有有意义的核心和 CLI 测试。

验证：完整仓库校验；运行 release CLI 的 help、version、capabilities 和自生成小型 PNG/JPEG 的 info/run；对 JSON 结果和输出解码像素作断言。

## 完成记录

状态：done，2026-09-19。依据用户最新范围，智能编辑只标为未来 roadmap，没有模型接入或选型；直接编辑、图层/文字、ops 工程历史和预览的后续目标保留。未修改 plan.md 或其他 todo。

1. 建立 `pic-core` / `pic-cli` Cargo workspace，真实 help/version/capabilities/info/run/identity 可执行；能力查询仅把 identity 列为可执行操作，区分 supported、partial、not_implemented，以及 planned/roadmap。
2. `Pipeline::single` 与 JSON 管线进入相同 `pipeline::run` / `Operation::apply`；测试验证逻辑步骤顺序、f32 位模式不变、管线目录资源解析、单操作/管线等价像素及所有 JSON 成功/失败响应。document 只预留 Raster/版本/稳定 ID 契约，未提前实现历史或预览。
3. 真实 PNG/JPEG 空管线、格式转换、灰度/透明度、非法文件/格式/JSON/参数/版本/目标/资源限制均有测试。PNG 全部 256 通道级和隐藏透明 RGB 精确保留；JPEG 固定样本检查实际解码像素误差。原子发布覆盖并发不覆盖竞争、显式覆盖、部分写入故障注入及旧目标保留。
4. [执行契约](../../../../docs/foundation-contract.md)、[功能矩阵](../../../../docs/capability-matrix.md)、[本机基线](../../../../docs/baseline.md) 记录了精度/alpha/坐标/资源/EV/倍率、schema/revision/op_version/stable target、实际硬件与剩余未知事项。基线每组 30 次新进程，素材和输出均在临时目录；不宣称 Photoshop 完全兼容或照片级性能。
5. `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --release`、`git diff --check` 全部通过。workspace 共 24 个测试（14 CLI + 9 核心集成 + 1 发布故障单元测试）；另以 `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli` 验证实际 release 二进制，14 个测试全部通过。

完整命令与逐项测试名称见 [基础验收记录](../../../../docs/foundation-validation.md)。无剩余 blocker；ICC/EXIF/动画/高位深、大图与跨平台性能、几何/图层/历史/预览均按契约留待后续任务。任务仅保留一个本地 commit；实施阶段未 rebase，集成阶段依用户指令 rebase 到 main，未 merge、push 或创建 PR。
