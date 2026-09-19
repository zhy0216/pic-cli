# 自包含工程与持久化历史

任务 04 实现单图 `.pic` 工程：保存原始素材和不可变语义 ops，通过原有 `Pipeline` / `Operation` 完整重放任意已提交步骤。RGBA32F 的负值、高亮、alpha 和逻辑边界与直接管线相同；只在最终导出/预览编码时量化。没有第二份可编辑文档状态、像素检查点或预览缓存。

## 命令与并发约定

```sh
pic-cli project create --input photo.png --output work.pic --json
# three.json 含三步，返回 r1、r2、r3；一个组 c1，当前指针 r3
pic-cli project apply work.pic --pipeline three.json --expect-revision r0 --json
pic-cli project inspect work.pic --revision r2 --json
pic-cli project preview work.pic --revision r2 --output step.png --json
pic-cli project undo work.pic --expect-revision r3 --json   # 回到 r0
pic-cli project redo work.pic --expect-revision r0 --json   # 回到 r3
# 从组内步骤 r1 续编；expected_revision 仍比较当前 r3
pic-cli project apply work.pic --pipeline followup.json --revision r1 --expect-revision r3 --json
pic-cli project export work.pic --revision r3 --output old-result.png --json
pic-cli project export work.pic --output current.png --json
```

`three.json` 的一个完整例子：

```json
{
  "schema_version": 1,
  "operations": [
    {"op":"resize","op_version":1,"target":"canvas","params":{"width":640,"filter":"bilinear"}},
    {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":1,"brightness":0,"contrast":1,"saturation":1}},
    {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":-1,"brightness":0,"contrast":1,"saturation":1}}
  ]
}
```

`followup.json` 可为单步 flip。所有 JSON 参数遵守已有管线契约，操作经校验转换为规范化规格再记录，包括 resize 的 null 缺省维度。启用 JSON `float_roundtrip` 保持序列化前后参数 f64 精度。空 apply 返回 `invalid_argument`，显式 identity 则是有效编辑步骤。

| 命令 | 选择与副作用 |
| --- | --- |
| create | 新 `.pic` 目录必须不存在；保存一份原始编码素材，初始 revision 为 r0；没有编辑 ops |
| apply | `--expect-revision` 必填；`--revision` / `--from-revision` 可选择任意已提交步骤，缺省用当前指针；整个管线成功后提交为一组 |
| inspect | 缺省当前 revision；返回选中步骤的画布尺寸、稳定 canvas ID、op/group ID、完整历史记录、活动 revision 集合与有序组边界；不移动指针 |
| undo / redo | `--expect-revision` 必填；只沿当前活动路径跨一个整组边界；不增加 ops |
| export / preview | 缺省当前 revision；都只读，使用统一 codec 及 PNG/JPEG 编码/覆盖参数；输出必须在该工程目录之外 |

`--expected-revision` 是 `--expect-revision` 的别名。expected_revision 与编辑基点分开：若当前 r3，读取 r1 后用 `--expect-revision r1` 修改会冲突；必须显式使用当前 r3，同时以 `--revision r1` 选择基点。锁内读取当前 manifest，并在最终发布前再次比较 expected_revision 和整个原 manifest 快照。并发同一 expected revision 的 apply 只有一个成功，失败者返回 `revision_conflict`。

每一步获得项目内稳定且不复用已提交编号的 `opN` / `rN`；每组为 `cN`。调用方应使用返回 ID，不能把编号当活动历史索引。一次三步组的 undo 从 r3 回 r0，但 r1/r2 始终可读取。若从 r1 追加一步得到 r4，活动组边界变为 r0 → r1 → r4；undo r4 回 r1，再 undo 回 r0。原 c1/r2/r3 文件不变且可读取，redo 只沿新路径走，不再回原 r3。尚未提供命名分支、合并或原地修改旧步骤。

## 权威文件与验证

```text
work.pic/
  manifest.json            # schema、源资产、当前/活动 tip revision、全部已发布 commit 索引
  .lock                    # 稳定锁 inode；写入端不得替换或删除
  assets/<sha256>           # 原始 PNG/JPEG 文件字节，不依赖扩展名
  ops/<sha256>.json         # 不可变组记录；只有 manifest 引用的才已提交
```

manifest 的 `source={sha256,bytes}` 引用内嵌原始素材；`imported_from` 仅供溯源，绝不用于恢复时找文件。相同文件字节只存一份，已存在资产重新校验后复用；不同编码的同像素图片不视作同一文件资产。每个 ops 文件包含结构版本、commit ID、组 base/output revision 和 `steps` 数组；步骤包含 op ID、base/output revision、规范化 `operation={op,op_version,target,params}`、输入与结果资产引用。目前直接单图操作的输入依赖都是源资产，结果资产为空，后续图层任务扩展同一结构。

打开时只读取 manifest 索引，校验所有已提交 ops（包括失效 redo 路径），其摘要、结构版本、操作语义版本、ID 唯一性/顺序、前向 revision 链、组边界与规范化参数。选中步骤恢复时验证源素材长度和 SHA-256 后，直接解码这些已验证字节，避免在哈希验证与解码之间再次按路径读取。修改素材或 ops 字节会得到 `integrity_mismatch`；缺必需素材为 `asset_missing`；缺 ops 为 `file_not_found`。未知结构/op/像素语义版本为 `unsupported_version`，不使用新默认值猜测历史。engine_version 记录创建版本，重放兼容性以结构/op/像素语义版本校验。

已有只读 `Project` 对象保持打开时的 manifest 快照；下一次 CLI 调用重新打开获取最新状态。结构查询的画布信息同样经过真实重放，不保存与 ops 独立的尺寸状态。

## 发布、路径与预算

写入操作持有 `.lock` 的操作系统排他锁直到完成；锁随进程关闭/退出释放，不使用容易遗留的 PID 锁文件。只读操作不等写锁。创建工程在同父目录临时目录里完成资产与 manifest，最后通过 no-clobber rename 一次发布；即使竞争者创建了空目录也不覆盖。

apply 完成校验、全量恢复和新操作执行后，检查总预算，再完整写入不可变 ops，最后原子替换 manifest。素材、ops、manifest 使用目录内临时文件写入、flush、关闭、rename；普通错误清理临时文件。执行、资产写入、ops 写入或 manifest 发布失败不改变原已发布历史。manifest 发布前留下的未引用 ops/资产不成为成功历史；强制终止可能遗留临时目录/文件或未引用记录，目前不做垃圾回收。

内部读写通过固定目录句柄和 `openat` / no-follow 访问，资产与 ops 引用仅接受 64 位小写 SHA-256，不接受绝对路径、`..` 或任意文件名。工程根目录、内部目录、manifest、素材、ops、锁文件的符号链接被拒绝；文件必须为普通文件。输入素材按普通 codec 规则读取并内嵌，允许输入指向普通文件的符号链接。project export/preview 拒绝规范化后位于该工程内的路径，避免覆盖权威文件。

| 准入项 | 默认值与统计范围 |
| --- | --- |
| 源素材 | 128 MiB 编码字节；沿用 codec 尺寸/缓冲限制 |
| 单次 apply | 沿用 1 MiB 管线文件、10,000 步限制 |
| 历史操作数 | `max_history_operations=100,000`，包含已失效路径 |
| 工程元数据 | `max_project_bytes=64 MiB`，manifest 加全部已提交 ops 文件字节；不含素材及未引用文件 |

apply、undo、redo 都在发布前检查新 manifest 加已有/新增 commit 的**总**预算；库调用可收紧预算，即使只是 revision 字符串变长或 compact manifest 转为 pretty JSON 也不能发布超限工程。失败保留原 manifest 字节。资源准入不是进程 RSS 限制。

当前实现使用 `rustix` 的 POSIX 目录句柄、flock 和支持 no-clobber 的 rename，已在本机 Linux 验证；其他平台尚未验收，Windows 存储后端尚未提供。不执行 fsync，不承诺突然断电恢复或任意网络文件系统的锁/rename 语义。调用方收到 stdout 写出失败或进程在发布后退出时，应重新 inspect 判断提交结果。工程搬移应在编辑进程结束后进行。

## 工程模式计时

工程命令沿用结果 envelope 中现有的毫秒字段，同一字段累加本次调用的多个计时块，失败的计时块也计入；未执行的阶段为 0。各字段按当前代码的计时范围归属：

| 字段 | 实际归属 |
| --- | --- |
| `validation_ms` | CLI apply 的管线文件读取、解析与参数校验；export/preview 的编码选项、输出路径、覆盖及工程内输出限制检查 |
| `read_ms` | create 的原始图像路径解析与字节读取；`Project::load` 的 manifest/ops 读取、JSON 解析、摘要/版本/参数及历史关系校验；`restore` 的源资产读取、哈希与长度校验 |
| `decode_ms` | 原始输入或已验证源资产的图像准入、解码、EXIF 归一化及工作像素转换 |
| `process_ms` | 恢复选定 revision 的管线构建与完整重放，以及 apply 的新操作执行；inspect、undo/redo、export/preview 所需的重放也计入 |
| `encode_ms` | export/preview 的最终像素量化、透明度处理和图像压缩编码 |
| `write_ms` | 工程资产/ops/manifest 的持久化、临时文件/目录创建、flush、关闭与原子发布，以及 export/preview 的输出发布；存储时的摘要计算和发布前 manifest 重读、解析、expected_revision/快照复核也在该计时范围内 |
| `total_ms` | CLI 从 main 入口到结果序列化前的全部耗时，包括 CLI 解析、工程写锁等待和未分类开销；不包含进程启动、stdout 序列化/写入及进程退出 |

写锁等待没有单独阶段字段。计时块外的工程路径处理、历史路径选择、记录构造、提交前 JSON 序列化、预算检查等开销只计入 `total_ms`；因此各阶段之和不等同于 `total_ms`，也不能替代父进程测量的端到端时延。库接口仅累加阶段耗时，`total_ms` 由调用方设置。

## 验收证据

核心测试位于 `crates/pic-core/src/project/tests.rs`，真实独立 CLI 进程测试位于 `crates/pic-cli/tests/project.rs`。测试使用临时目录自动生成素材；故障注入只编译进单元测试，没有生产环境变量或 CLI 故障开关。

| 验收条件 | 自动验证 |
| --- | --- |
| 单图、整组、每个中间 revision、undo/redo、旧步骤续编与导出跨进程完成 | `groups_intermediate_steps_undo_redo_forks_and_exports_across_processes`；另测撤销后续编丢弃旧 redo |
| 失败、查询、预览不进入历史 | `failed_operations_queries_and_preview_never_enter_history`；比较 manifest 原始字节和 ops 数量，执行到第二步失败也不发布 |
| 移动、删除源文件/管线、去重与明确损坏错误 | `moved_project_needs_neither_original_image_nor_pipeline_files`、资产去重单测、缺失/损坏/未知版本与图验证测试 |
| 并发与失败原子性 | 两个 CLI 竞争相同 expected_revision、两个 CLI 竞争创建；资产/ops/manifest 半写入及发布前故障；旧 manifest 与已恢复像素保留 |
| 路径限制 | 遍历/绝对引用、资产/目录/工程 symlink、工程内导出拒绝 |
| 精度与统一语义 | 跨提交逐步骤比较直接管线与重放的每个 f32 位；真实 PNG 导出解码像素一致，含负值/HDR、透明色与插值 |
| 总预算边界 | `undo_redo_account_for_all_commit_bytes_before_replacing_manifest`：r0→r10 与 compact→pretty 均在紧预算下失败，原 manifest 完全不变 |

全仓库校验与 release CLI 验收命令见 README。检查点、局部/缩放预览、坐标映射、参数修改模板属于任务 05；本任务的 preview 仅提供指定步骤的全尺寸只读导出。
