difficulty: hard
agent: inherit

# 自包含工程与持久化历史

## T1 · 资产、不可变 ops 和跨进程原子提交

前置依赖：03-adjustments-filters.md。

要做什么：实现 .pic 目录的 manifest/ops/assets，内容哈希去重导入素材，结构版本与操作语义版本验证。记录已规范化语义 ops、稳定目标/op ID、base/output revision 和组提交边界；工程状态只能由原图/已提交 ops/必需结果资产恢复。实现 project create/apply/inspect/export、线性 undo/redo、指定已提交步骤读取和继续编辑；旧 revision 不可原地改写，旧 redo 路径失效但历史只读可追溯。发布前校验 expected_revision，跨进程写锁或等价原子冲突控制；失败不得发布半套历史。工程路径与素材引用不得逃逸预期资产范围。

预计修改：pic-core document、资产/历史/存储模块与 pipeline 接口，pic-cli project 子命令及文档、集成测试，本 todo/README。

验收条件：

- [x] 单图导入、组提交、多步中间 revision 读取、撤销/重做、从旧步骤续编和导出全部可跨独立 CLI 进程完成。
- [x] 查询、失败操作和预览不进入编辑历史；只有成功发布的 ops 成为权威状态。
- [x] 移动工程并删除原始导入文件仍能恢复；素材去重，丢失/损坏素材、未知 schema/op 版本有清晰错误。
- [x] 并发 expected_revision 冲突只有一个发布成功；执行/资产写入/manifest 发布失败不产生部分可见历史，保留原有效工程。
- [x] 直接管线与从工程 ops 全重放的解码像素一致，完整仓库校验通过。

验证：完整仓库校验；跨进程集成测试、并发提交、故障注入/受控 IO 失败、素材移动与哈希损坏测试。

## 完成证据

- 实现：`pic_core::project` 在现有 `Raster`、`OperationSpec`、`Pipeline` 与 codec 上增加 manifest、按 SHA-256 保存的资产与不可变组记录；CLI 提供 create/apply/inspect/export/preview/undo/redo。preview 限于全尺寸步骤读取，检查点、区域/缩放与模板仍属于 05。
- 跨进程闭环：`crates/pic-cli/tests/project.rs` 的 `groups_intermediate_steps_undo_redo_forks_and_exports_across_processes` 验证组内 r1/r2/r3、整组撤销/重做、从旧步骤续编、只读导出旧路径与不可变 ops 字节；另测 undo 后续编使 redo 指向新路径。
- 历史纯净：失败管线第二步、空 apply、未知版本、错误 revision、inspect 与 preview 均不改变 manifest 和已提交 ops 数量；未被 manifest 引用的故障遗留 ops 不进入历史。
- 自包含与完整性：移动工程后删除原图和管线仍能恢复；重复资产导入仅保存一份。缺失/损坏源资产、缺失/损坏 ops、未知 schema/op/像素版本、非法图关系、路径遍历/符号链接均有回归。
- 原子性：两个独立 CLI 竞争相同 expected_revision，仅一个成功；并发 create 不覆盖赢家。仅单测启用的资产/ops/manifest 半写入及发布前故障注入验证原 manifest 和可恢复状态保留。发布前重新核对 expected_revision 与 manifest 快照。
- 协调器指出的预算边界已修复：apply/undo/redo 共用总元数据预算检查；`undo_redo_account_for_all_commit_bytes_before_replacing_manifest` 覆盖 r0→r10 和 compact→pretty 扩容，紧预算下失败并保持 manifest 原字节。
- 精度：直接执行与跨提交全重放的每个 f32 位一致（含 HDR、负值、透明度、曲线与缩放），CLI 导出 PNG 的解码像素一致；未生成量化中间状态。

校验结果（本任务 worktree，本机 Linux）：

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace` | 77 通过，0 失败；新增工程测试 19 项 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过 |
| `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli --test project` | release 二进制 35 项 CLI 回归通过 |

无外部 blocker。存储后端在 Linux 验收，未提供 Windows 后端；不执行 fsync，不承诺断电持久性。强制退出可能遗留不参与权威历史的临时文件或未引用记录。完整格式、命令和测试映射见 [工程契约](../../../../docs/project-history.md)。
