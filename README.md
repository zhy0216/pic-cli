# pic-cli

面向 AI agent 的 Rust 图像编辑 CLI。调用方选择明确操作和参数，CLI 本地执行；普通编辑、图层/文字与自包含 ops 工程都不需要 LLM、图像模型或凭据。

当前 **0.1.0** 已提供 PNG/JPEG、几何、调色/滤镜、稳定 ID 图层与蒙版、隔离组/剪贴/文字/点调整层，以及跨进程历史、预览坐标映射、expected_revision 续编和单画布模板。智能抠图、主体分割、修复、生成填充/扩图保留在 [未来 roadmap](plans/fast-image-editing/roadmap/README.md)，尚未实现，也没有配置模型即可启用的后端。

## 构建与开始使用

已验证 Ubuntu 24.04 / Linux x86_64 / glibc 2.39，Rust/Cargo **1.98.1**。清单声明的 `rust-version=1.88` 尚未验证；其他平台也未验证。仅需锁定的 Rust 依赖，不依赖 reference/ 或原生图像库。

以下构建命令在完整源码 checkout 运行。已解包的二进制包直接使用 `bin/pic-cli`，独立验收入口见 [打包指南](docs/packaging.md)。

```sh
cargo build --locked --release
target/release/pic-cli --help
target/release/pic-cli --version
target/release/pic-cli capabilities --json
```

外部 agent 从 [执行指南](docs/agent-guide.md) 开始：它提供可生成的素材、带许可的明确字体绑定、真实命令和 [JSON 示例](examples/README.md)，完整走通普通处理、创建工程、观察坐标、继续编辑、undo/redo、模板换图及图层文字闭环。指南中的关键命令块由打包验收直接执行，避免示例语法与实现漂移。

发布式构建、打包并在独立临时目录验收：

```sh
python3 scripts/package.py
```

需要 Python 3.12+ 标准库、sh、ldd、Git 和 Rust；产物位于 `target/dist/pic-cli-<随机后缀>/`，包含 tar.gz、`bin/pic-cli`、校验和、编译信息、字体许可及完整验收证据。脚本解包实际压缩包后运行指南、错误恢复和已有图层里程碑（含 4K 双层）；不依赖源码 cwd、外部导入文件或 reference/。二进制运行本身不需要 Python/Rust。安装、动态库与平台限制、包的单独重跑方式见 [构建/安装/打包](docs/packaging.md)，本轮结果见 [验收记录](docs/packaging-validation.md)。

## Agent 调用约定

- `--json` 可放在子命令前后，stdout 恰好一个版本化 JSON 对象，help/version/解析错误也适用。成功 0、执行错误 1、CLI 解析错误 2；按 `ok` 和 `error.code` 判断。完整说明见 [错误与恢复](docs/errors.md)。
- `capabilities --json` 返回可执行 `operations`、参数 schema、语义、限额与 `supported / partial / not_implemented` 状态。版本：结果、pipeline、document schema 均为 1，各操作 `op_version=1`；它们分别管理，不要与 revision 混用。
- 图像处理命令用 `--input` / `--output`，`info` 的输入是位置参数。CLI 路径相对 cwd；管线素材路径相对 JSON 文件的规范化父目录。输出父目录须存在，覆盖普通文件需 `--overwrite`。
- 一次管线按数组顺序执行；每步坐标使用当前画布，左上原点、x 向右、y 向下，像素中心 `(x+0.5,y+0.5)`。线性 sRGB RGBA32F 保留负值/高亮和 alpha，最终导出才量化。
- `.pic` 保存原始素材、字体与不可变 ops；manifest 发布当前历史指针，检查点/预览缓存是可删除的加速副本。不要修改 ops 或用扁平 PNG 代替完整工程状态。
- 工程编辑、undo/redo 必须带 `--expect-revision`。从旧步骤继续用 `--revision OLD --expect-revision CURRENT`；读旧步骤不移动指针。冲突时重新 inspect/preview/分析，不能盲目重试旧坐标。

## 能力与明确限制

| 范围 | 当前能力与边界 |
| --- | --- |
| 输入/导出 | 静态 8-bit sRGB PNG/JPEG 子集，EXIF 主方向 1–8 归一化；ICC、高位深、调色板 PNG、动画、PSD/RAW 不支持。导出不保留元数据；透明 JPEG 需显式背景 |
| 几何/调色 | crop、nearest/bilinear resize/rotate、flip、canvas；EV/亮度/对比度/饱和度、色阶、折线曲线、灰度、反相、高斯/unsharp。参数不等同 Photoshop 滑杆 |
| 图层/蒙版 | 稳定 ID、显隐/排序/opacity、无损变换、normal/multiply/screen/overlay；外部 coverage 蒙版、显式空间矩形选区、单层/蒙版预览 |
| 组/文字/调整 | 隔离有界组、同级下方 clip、五种点调整层；显式静态 TrueType、LTR Latin/Greek/Cyrillic 子集、LF 换行和固定框。无中文/bidi/自动换行/字体回退/pass-through 组 |
| 工程/模板 | 整组提交/撤销/重做、任意已提交步骤恢复/续编、完整浮点检查点、旧参数修订。模板仅支持显式重绑定输入/目标/全部参数的单画布操作；图层树/蒙版/字体等模板拒绝 |
| 智能能力 | 未实现，future roadmap；本轮没有模型调用、模型依赖或配置要求 |

详细参数与语义按工作类型阅读：

| 工作 | 文档 |
| --- | --- |
| 几何、格式、EXIF、编码、颜色/alpha | [几何/codec](docs/geometry-codecs.md)、[执行与版本契约](docs/foundation-contract.md) |
| EV、倍率、色阶/曲线、滤镜公式 | [调色与滤镜](docs/adjustments-filters.md) |
| 资源持久化、锁、提交与历史 | [工程历史](docs/project-history.md) |
| 中间观察、坐标映射、缓存预算、模板 | [重放与预览](docs/replay-preview.md) |
| 图层/蒙版/选区、组/剪贴/文字/调整 | [图层](docs/layers-masks.md)、[文字与里程碑](docs/groups-text-adjustments.md) |
| 状态与兼容性 | [功能矩阵](docs/capability-matrix.md) |

## 性能与验证

[当前性能报告](docs/performance.md) 保留 16 场景前后各 30 次 release 数据、1080p/4K、图层与工程重放/检查点/旧步骤修改、峰值 RSS 和逐像素核对。4K 调整 p95 为 1171.49 ms，未达到方案建议的 1 秒；没有 Photoshop/libvips 对照或跨平台时延承诺。[任务 01 小图历史基线](docs/baseline.md) 保留早期数据，不代表当前全部功能、性能或测试总数。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
git diff --check
python3 scripts/package.py
```

核心、管线、Document 与 Project 共用执行/渲染逻辑，位于 `crates/pic-core/`；CLI 解析和真实进程测试位于 `crates/pic-cli/`。`examples/` 保存可编辑 JSON，`scripts/` 提供打包和性能复跑入口，`target/` 为忽略的生成物；只读 `reference/` 不参与构建或验收。扩展操作应沿用统一核心，不增加平行状态源。
