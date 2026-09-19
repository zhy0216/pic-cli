difficulty: hard
agent: inherit

# Agent 文档、示例和基础打包验收

## T1 · 让外部 agent 可发现并可靠执行现有能力

前置依赖：11-core-performance.md。本轮交付只包含直接编辑，智能功能仅标记为未来规划。

要做什么：完善根 README、能力/参数/错误/恢复指南、可运行命令和 JSON 管线/操作模板；整理 P0 中间状态分析与续编、图层/蒙版/文字闭环示例。建立实际可验证的安装/构建/打包途径与检查，记录已验证平台和未验证平台；确保 help/capabilities/JSON 版本与代码一致。用独立临时目录跑打包后的二进制，避免依赖 repo cwd/reference/外部导入文件。智能能力按实际状态说明，不能因缺模型而隐去需求。

预计修改：README/docs/examples、打包/验收脚本与必要 CI 配置、pic-cli 帮助/发现性修复、相关测试，本 todo/README。

验收条件：

- [x] 外部 agent 可仅根据文档完成普通处理、创建工程、预览坐标映射、expected_revision 续编、撤销/重做/模板换图和图层里程碑。
- [x] 示例实际运行通过；JSON 解析、stderr/退出码、缺参数/素材/冲突错误及恢复说明与实现一致。
- [x] 发布式构建/打包通过，在临时目录用实际二进制独立执行；素材和字体依赖明确，测试不依赖 reference/。
- [x] 支持/部分支持/未实现/需模型配置区分准确，性能报告链接和已验证平台清楚。
- [x] 完整仓库校验通过；本轮直接编辑交付与未来智能功能边界明确。

验证：完整仓库校验；打包二进制运行全部文档关键示例及普通编辑里程碑。


## 完成记录（2026-09-19）

- [Agent 指南](../../../../docs/agent-guide.md) 的五个可执行命令块通过实际包内二进制运行，58 份 JSON 回执覆盖普通编辑、P0 坐标/续编、undo/redo/旧步骤/修订、显式模板换图及图层/蒙版/组/文字/调整/clip。
- 全部 48 个可发现命令的 JSON help 与版本核对通过；额外 71 次调用含 16 次预期 JSON 失败及一次文本错误，验证缺参数/图片/字体、冲突、缺绑定/模板限制、缺字、资产缺失/损坏、stderr/退出码及失败无发布。10 对解码像素比较（741,376 通道）差异为零。
- [打包脚本](../../../../scripts/package.py) 用锁定依赖 release 构建、生成 tar.gz 并重新解包，在独立临时 cwd 执行文档及已有 `layer_milestone --4k`；临时复制安装、移动工程并删除外部素材/字体后重放均通过。字体来源/许可随包提供，不依赖 reference/。
- 输出目录 guard 在 mkdir/build 前解析绝对路径和父目录符号链接，拒绝等于或位于四个复制源树内的路径；12 个拒绝用例通过，无递归复制。默认 target/dist 和外部 /tmp 路径可用，外部目录完整打包已通过。
- 当前支持/子集/未实现/无可配置模型后端、Linux 及 Rust 1.98.1 实测/1.88 声明下限未测、格式/排版/模板限制已明确；旧基线数字保留并链接当前性能报告。plan.md 未修改，其他 todo 状态保留。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（129 项）、`cargo build --release`、`git diff --check` 全部通过。完整 [验收证据与复跑](../../../../docs/packaging-validation.md)、[安装/生成物布局](../../../../docs/packaging.md)。

main checkout 合入后可重跑 `python3 scripts/package.py`，默认生成物在 `target/dist/pic-cli-<随机后缀>/`。也支持 `--output /tmp/pic-packages`；成功的 package-result.json 指向 archive、实际二进制及全部验收证据。最终产物绝对路径见任务交接汇报，不能依赖独立 worktree 永久存在。

无外部 blocker。其他平台/MSRV 未验证；4K 调整超过建议 1 秒、无 Photoshop/libvips 对照等已披露性能限制保持原报告结论；智能编辑仍仅为未来 roadmap。
