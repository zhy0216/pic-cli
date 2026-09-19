difficulty: hard
agent: inherit

# 首版契约和 Rust 基础骨架

## T1 · 建立可编译、可扩展的实际处理入口

前置依赖：无。

要做什么：读取方案和 AGENTS.md，建立 pic-core 库与 pic-cli 二进制的 Cargo workspace。用当前机器作为首轮开发与性能基线，文档化 PNG/JPEG、8-bit sRGB 输入、内部像素精度与 alpha、坐标、资源限制、EV/倍率等参数语义及三类能力对齐矩阵；未实现能力必须如实声明。内部表示须支持后续检查点无损恢复，不能在步骤间无意降低精度。建立 codec、operation、pipeline、document 的清晰边界、统一错误/结果版本和阶段计时。CLI 至少提供 help/version/capabilities/info/run，run 可执行空操作管线完成真实读写，未知操作拒绝。采用 image 起步；依赖选择以本机实际可构建为准，不引入多套完整后端。记录 schema_version/revision/op_version/stable target 的契约，后续实现不在本任务展开。

预计修改：待建 Cargo.toml、Cargo.lock、crates/pic-core/、crates/pic-cli/、README.md、docs/ 下基础设计/功能矩阵；.gitignore 增加构建产物；本 todo 及队列状态。这是仓库首次源码创建，最终布局应在 README 中明确。

验收条件：

- [ ] 真实 Rust workspace 编译通过，CLI 的 help/version/info/capabilities/run 可调用；能力查询区分支持、部分支持和未实现。
- [ ] 单操作与管线复用核心接口；管线顺序和资源相对路径（管线目录）有明确契约；JSON stdout 仅输出稳定版本结果，日志走 stderr，非法输入为非零退出码。
- [ ] PNG/JPEG 最小真实输入输出、空管线、文件/格式/JSON/参数错误及覆盖控制有集成证据；输出以临时文件后原子发布，失败不留下成功目标。
- [ ] 文档给出执行语义、基准机器信息和剩余未知事项；未声明 Photoshop 参数/文件完全兼容或未经测量性能。
- [ ] 运行队列 README 中全部仓库校验，并有有意义的核心和 CLI 测试。

验证：完整仓库校验；运行 release CLI 的 help、version、capabilities 和自生成小型 PNG/JPEG 的 info/run；对 JSON 结果和输出解码像素作断言。
