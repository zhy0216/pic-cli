difficulty: hard
agent: inherit

# 重放、检查点、观察与继续编辑

## T1 · 精确恢复、缓存失效和版本化预览

前置依赖：04-project-history.md。

要做什么：按需生成有磁盘/内存预算的检查点与派生预览缓存；身份包含源资产、规范化操作前缀、全部依赖、语义/颜色/采样版本，预览另含目标、区域和尺寸。恢复不能量化内部像素精度，缩略预览不能用作导出检查点；缓存缺失/失效时安全回退。支持指定 revision 全图/局部预览、输出画布/区域/预览尺寸与坐标映射。实现修改旧步骤参数产生新历史及有效前缀复用；导出/运行显式输入槽和参数的操作模板，内容相关蒙版/模型结果不能静默跨输入复用。

预计修改：pic-core document/pipeline/checkpoint/cache/preview（按真实布局），pic-cli project 命令，docs 和测试，本 todo/README。

验收条件：

- [x] 检查点恢复、删除全部派生缓存后的全重放、直接管线均得到一致像素和状态；损坏检查点安全回退，必需资产不被预算淘汰。
- [x] 20 步历史修改第 18 步与第 3 步时正确复用前缀/失效后缀，报告命中与重算步骤，原有 revision 内容不变。
- [x] 预览任意已提交步骤和裁剪区域，坐标映射可逆核对；分析后 stale expected_revision 被拒绝，缩小预览不影响全尺寸导出。
- [x] 跨新进程完成 P0 里程碑：编辑→检查点→退出→中间预览→续编→导出→清缓存重放。
- [x] 模板换图要求必要的目标/坐标/素材重新绑定，不复用旧 revision/内容相关输出；完整仓库校验通过。

验证：完整仓库校验；新进程里程碑、缓存预算/损坏/内容变化、精度与坐标测试。

## 完成记录

- 沿用 Project/Pipeline/Operation/Raster；按需完整 RGBA32F 检查点、独立区域/缩放预览缓存、SHA-256 前缀身份、依赖验证、磁盘/内存预算与损坏回退已实现。当前完整状态是单画布，图层任务后续扩展同一核心。
- 第 18 步与第 3 步修改在核心和独立进程测试中分别复用 17/2 步、重算 3/18 步；旧 ops 文件、revision 像素和撤销边界保留。读取/修改报告 cache_hit、reused_steps、recomputed_revisions。
- 浮点检查点测试逐位核对 HDR、负值、alpha、隐藏 RGB、signed zero/subnormal；零字节/截断/损坏/错误版本缓存安全回退。协调器指出的零长度清理边界已修复，聚焦测试覆盖仅零字节与混合损坏文件，assets/ops/manifest 保持不变。
- 预览任意已提交 revision，输出 canvas/region/preview_size 和双向连续像素坐标映射；stale expected_revision 拒绝；缩略缓存不进入完整导出恢复。P0 新进程闭环清缓存后与直接管线的 PNG 解码像素一致。
- 模板导出槽位及建议参数；新输入/目标/每步完整参数必须显式绑定，在新工程形成独立历史。当前源素材通过 input 绑定；外部蒙版、内容相关结果、未知模型操作/字段因尚无绑定契约而明确拒绝，不静默继承。运行失败不发布半个新工程。
- 验收说明和逐项测试名：[重放与预览契约](../../../../docs/replay-preview.md)。核心测试：project/tests/replay.rs、project/cache.rs；独立进程测试：pic-cli/tests/project/replay.rs。

验证结果（当前任务 worktree）：

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace` | 90 项通过 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过 |
| `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli --test project` | 40 项 release CLI 回归通过 |

无外部 blocker。平台仍为已验证的 Linux/POSIX；没有 fsync/严格进程 RSS/大图性能承诺。模型及多图层实现不在本 todo 范围内。
