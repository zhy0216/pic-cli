# 任务 12：直接编辑交付验收

2026-09-19，Linux x86_64 / Ubuntu 24.04 / glibc 2.39，rustc/cargo 1.98.1，Python 3.14.7。这里记录当前任务结果；任务 01 的 24 项测试、任务 02/03 等阶段数字仍是历史证据。当前 workspace 共 **129 项测试**，不是 24 项。本任务没有改变处理、渲染、历史或缓存算法，也未改依赖锁。

## 验收对应关系

| 条件 | 实际证据 |
| --- | --- |
| 外部 agent 可独立完成普通编辑、P0、历史、模板与图层 | [agent 指南](agent-guide.md) 五个带标记命令块被验证器直接提取执行，产生 58 份单行 JSON 回执。覆盖所有普通编辑命令、两步 apply、r2 检查点、32×16 区域预览、坐标中心映射到 `(15,15)`、expected revision 续编、整组 undo/redo、r1 旧步骤续编、revise、新输入模板与完整图层/蒙版/文字闭环 |
| JSON、stderr、退出码及错误恢复一致 | 48 个 capabilities 命令的 JSON help 全部成功；结果/管线/操作版本与二进制一致。额外 71 次调用记录 argv/退出码/stdout/stderr，含 16 次预期 JSON 失败和 1 次无 `--json` 的失败。缺参数、缺图/字体、非法 JSON/版本/操作、冲突、redo 边界、模板限制/缺绑定、缺字、缺失/损坏内嵌字体均准确报错；失败后 manifest 和已有输出字节保持不变 |
| 发布式构建、安装、打包及独立执行 | [package.py](../scripts/package.py) 用 `--locked --release` 构建 host 二进制和 Rust 验收程序，生成 tar.gz/校验和/构建元数据，重新解包实际 archive 后在另一临时 cwd 执行。`install -m 755` 到临时个人目录后版本与二进制摘要一致。工程移动、删除外部图片/蒙版/字体/管线、清缓存后可继续修改和完整导出 |
| 像素与图层里程碑 | 10 对实际解码 RGBA8 比较，共 741,376 个通道，差异为 0：直接管线/工程、undo/redo、保留旧版本、修订后冷重放、模板换图、图层 preview/export/replay 与资产恢复。额外复用 `layer_milestone --4k`：文字修改、蒙版覆盖 0/128/255、移动工程、字体丢失、完整检查点/冷重放及普通 3840×2160 双层的 direct/project 像素相等 |
| 能力/平台/性能边界准确 | 根 README、[功能矩阵](capability-matrix.md)、help、参数 schema 和 [错误指南](errors.md) 对齐。Linux/Rust 实测版本与未测平台/MSRV 分开；[历史基线](baseline.md) 保留原值并指向 [任务 11 性能](performance.md)。智能功能仍明确列出 future roadmap，没有模型配置或调用 |

图片由纯 Rust 程序在临时目录生成；DejaVu Sans 2.37 的原始字体、SHA-256、来源和许可随包提供，见 [字体说明](../tests/fonts/README.md)。运行时不使用系统字体、外部导入文件、reference/ 或仓库 cwd。原始素材/不可变 ops 仍是权威，检查点只作派生加速。

## 运行命令与结果

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，包含新增 agent_fixtures example |
| `cargo test --workspace` | 129 项通过 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过；归档/暂存后再检查最终 diff |
| `python3 scripts/package.py` | 通过；实际 tar.gz 解包、58 份指南回执、48 命令发现性、10 对像素比较、复制安装及图层/4K 里程碑均通过 |
| `package.py --output` 路径拒绝检查 | 4 个复制源树各测相等、子目录、父目录符号链接，共 12 个路径退出 2；将 PATH 设为空工具目录证明在调用 rustc/Cargo 前拒绝，未创建输出、未触发递归复制 |

第一次脚本试跑暴露验收器误用模板字段 `steps`，已修正为真实的 `operations` 并重新通过；这是验收器修正，不是产品格式变更。独立审查指出输出目录可位于被复制文档树内，已在 mkdir/build 前统一解析绝对路径和父目录符号链接并拒绝该情形；正常 `target/dist` 和外部 `/tmp` 保留为可用生成物位置。

## 复跑与产物保留

在 main checkout 合入后直接运行 `python3 scripts/package.py`，或 `python3 scripts/package.py --output /tmp/pic-packages`。每次生成唯一目录；不要依赖本任务 worktree 的绝对路径。结构与重跑方式见 [打包指南](packaging.md)。成功的 `package-result.json` 指向 archive、二进制和 `verification/verification.json`；后者与 `receipts/`、`calls.jsonl`、解码比较和 `layer-milestone/report.json` 一起保留实际证据。验证失败时非零退出并保留失败日志，不产生成功的 package-result。

本任务的完整 Cargo 日志位于忽略的 `target/packaging-validation/`。最终包路径由任务交接汇报提供，协调器可保存该目录或在 main checkout 重跑同一脚本；包内 build-info 会记录实际来源 commit 和 dirty 状态，文档不写入无法在新 checkout 重用的硬编码二进制路径。

无外部 blocker。仍只验证当前 Linux GNU 平台；1.88 MSRV、其他架构/系统未验证。格式/布局/模板子集和无 fsync 持久性保证见各契约。任务 11 的 4K 调整 p95 1171.49 ms 仍高于建议 1 秒，没有 libvips/Photoshop 对照；本任务没有重新跑 30 轮性能或改变该结论。
