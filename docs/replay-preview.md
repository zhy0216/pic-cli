# 重放、精确检查点、预览与模板

任务 05 扩展既有单画布工程。源资产和不可变 ops 仍是唯一权威状态；读取、续编、修改参数、预览和导出共用原有 Pipeline / Operation / Raster。新增派生缓存不进入 manifest，也不改变 revision、整组撤销或 expected_revision 规则。任务 06 在同一路径新增图层、蒙版、选区及完整状态检查点，详见 [图层契约](layers-masks.md)；任务 07 已增加组、剪贴、文字和点调整层，见 [对应契约](groups-text-adjustments.md)。模型操作仍只在未来 roadmap。

## 按需检查点与安全恢复

```sh
pic-cli project checkpoint work.pic --revision r2 --json
pic-cli project inspect work.pic --revision r2 --json
pic-cli project export work.pic --output final.png --json
pic-cli project cache-clear work.pic --json
pic-cli project export work.pic --output replayed.png --json
```

checkpoint 缺省选当前 revision，也可选 r0、组内步骤或已退出活动路径的旧 revision。apply/inspect/export 不自动为每一步写检查点；调用方可在观察或昂贵操作边界显式生成。preview 自动保存自己的观察结果，永不写入完整状态检查点目录。

恢复流程先校验 manifest/不可变 ops，再读取并核对必需资产的 SHA-256 与长度。即使命中进程内或磁盘缓存也必须验证资产，不能用缓存掩盖原图缺失、内容变化或未知操作版本。从选中步骤向前查找最近的有效完整检查点，然后通过统一管线执行剩余逻辑步骤；没有有效检查点时重新解码原始资产并完整重放。旧任务 04 工程无需迁移，缺少派生目录等同缓存未命中。

缓存键是规范化 JSON 的 SHA-256。起点包含源资产摘要/长度、工程结构版本、当前引擎版本、渲染/decoder/采样版本、线性 sRGB RGBA32F straight-alpha 语义和 OS/架构；每一步将上一前缀键、规范化 operation（含 op_version、稳定目标、所有参数及采样选项）、全部输入/结果依赖的摘要与长度继续哈希。无外部资源的 canvas 操作只直接引用源图；图层／蒙版操作显式记录新增资产，全部祖先依赖均参与验证。操作或依赖扩展必须沿用这些字段，渲染行为改变必须升级相应语义标识。revision/op/group 编号不作为内容身份，语义相同的前缀可以共享缓存。

完整检查点保存于 `checkpoints/<key>.bin`；预览缓存保存于 `cache/<key>.bin`。两者目录、键空间和读取入口独立。预览键还包含目标、区域、请求宽高和滤镜；缺省项有显式 null，JSON 字段顺序不影响键。预览缓存保留浮点观察结果，所以 PNG/JPEG 编码选项无需进入其身份，每次输出仍用请求的 codec 参数重新编码。

快照 v1 是 little-endian 二进制：8 字节 `PICFLT01`、64 字节 ASCII 身份摘要、画布宽高及栅格宽高（四个 u32）、逐像素 RGBA 四个 f32 原始字节，最后附 32 字节 SHA-256 校验和，覆盖此前所有字节。总长度为 `120 + 16 × 像素数`。恢复核对身份、版本、长度、校验和、尺寸/内存准入、样本有限性和 alpha 范围。负值、HDR、透明像素隐藏颜色、signed zero 和 subnormal 位模式均保留；没有 PNG/JPEG 中间量化。单画布检查点须覆盖完整画布；多层状态采用 PICDOC03 保存独立图层、蒙版和完整元数据，多层前缀拒绝扁平快照，格式见 [图层契约](layers-masks.md)。

缺失、截断、错误身份、未知快照版本、损坏或超预算的派生数据均安全回退。源资产/ops 的错误仍明确失败。缓存写失败不改变已发布历史，返回 `cache_write_skipped` 警告；显式 checkpoint 返回 `disk_stored` 和 `memory_stored`，调用方不能把预算导致的 false 当作已保存。

## 预算、淘汰与命中报告

所有 project 子命令接受 `--cache-disk-bytes` 和 `--cache-memory-bytes`，可置于子命令前后。它们是本次调用的策略，不写入权威 manifest；库对应 ResourceLimits 同名的 `max_cache_*` 字段。

| 预算 | 默认值 | 行为 |
| --- | --- | --- |
| 合计派生磁盘 | 512 MiB / 536870912 字节 | checkpoints 与 cache 合计；按最早写入时间淘汰。新文件超过预算则跳过，保存/跳过保存时收紧已有占用 |
| 缓存内存 | 256 MiB / 268435456 字节 | Project 对象持有的浮点样本采用 LRU；快照读写按同时存在的序列化与像素缓冲作保守准入，先腾出缓存空间；同时受 max_buffer_bytes 约束 |

预算按受管理文件的逻辑字节及浮点样本计算，不是文件系统分配块数或严格进程 RSS；原始图像解码、执行器工作缓冲、元数据/缓存索引和调用方仍持有的 Raster 由已有资源规则管理。过大的检查点可以不写盘，恢复仍可执行；内存预算为零禁用内存缓存和快照 I/O，磁盘预算为零禁止磁盘命中/保存，内存可单独使用。打开/纯读取不主动淘汰磁盘文件；收紧预算后显式 checkpoint 或 cache-clear 执行清理。

独立 `.cache-lock` 串行化缓存发布与淘汰，不占用历史写锁。读取无须等该锁，被并发淘汰时回退；历史编辑仍持有 `.lock` 并在发布前重查 expected_revision 和 manifest 快照。派生目录及文件使用目录句柄和 no-follow 访问，符号链接不会让缓存写入或淘汰触及外部目录。

cache-clear 清除当前 Project 的内存缓存及两个派生目录内所有小写 64 位摘要名的 `.bin` 条目，包含零长度/截断文件。它不删除 assets、ops、manifest、锁文件或未知文件名；崩溃遗留的临时文件沿用工程既有边界，不纳入自动垃圾回收。返回 removed_bytes 是逻辑字节和，因此成功删除仅零字节文件时仍为 0。其他已打开 Project 的有效内存副本不受跨进程清理影响；新进程或当前对象清理后的恢复可完整重放。

结果中的 `replay` 包含：

| 字段 | 含义 |
| --- | --- |
| cache_hit | null，或 kind=checkpoint/preview、tier=memory/disk、key 及对应 revision |
| reused_steps | 命中复用的逻辑前缀步数，命中 r0 时为 0 |
| recomputed_revisions | 本次实际执行的历史逻辑步骤，按执行顺序列出；apply/revise 包括新提交的后缀 revision |
| operations_replayed | inspect/export/preview 的实际重放步数；预览命中为 0；临时区域裁剪/缩放不作为历史步骤计入 |

旧后缀缓存可以保留供旧 revision 读取；参数变化后新前缀键使它们在新路径上不可命中，不依赖物理删除实现失效。没有命中不表示失败，也不宣称所有图像都能因缓存加速。

## 区域预览与分析后续编

```sh
pic-cli project preview work.pic --revision r1 --target canvas --region 100,80,640,480 --width 320 --output region.png --json
pic-cli project apply work.pic --pipeline followup.json --revision r1 --expect-revision rCurrent --json
```

region 是该 revision 画布上的 `x,y,width,height`，半开矩形必须完整位于画布内。缺省为整图，尺寸均为正整数。width/height 都省略时按区域原尺寸输出；只给一个时沿用 resize 的保持宽高比、四舍五入语义；给两个时精确指定输出尺寸。filter 支持 nearest/bilinear，默认 bilinear。裁剪和采样通过同一操作核心执行，预览的临时变换不进入历史。目标支持 canvas、图层 ID 及 mask:<layer ID>；所有 region 仍使用画布坐标，另返回完整局部仿射映射，未知目标明确拒绝。

JSON 保留既有 output、revision、op_id、target、width/height、format 等字段，增加 canvas、region、preview_size、filter 和 coordinates。例如画布 800×600，region=(100,80,640,480)，preview_size=320×240：

```json
{
  "canvas": {"width":800,"height":600},
  "region": {"x":100,"y":80,"width":640,"height":480},
  "preview_size": {"width":320,"height":240},
  "coordinates": {
    "convention":"pixel_edges; centers=(index+0.5); x_right_y_down",
    "preview_to_canvas":{"scale":[2.0,2.0],"offset":[100.0,80.0]},
    "canvas_to_preview":{"scale":[0.5,0.5],"offset":[-50.0,-40.0]}
  }
}
```

两个方向都用 `output = input × scale + offset`（逐轴）。这是连续像素边界坐标；预览像素索引 (i,j) 的中心需先转成 (i+0.5,j+0.5) 再映射。逆映射可核对边界、中心和分数位置，不承诺从缩略像素重建原始细节。坐标只关联返回 revision；分析之后 apply/revise 的 expected_revision 必须等于当前指针，旧分析版本不会因缓存命中而绕过冲突检查。从旧步骤继续时显式提供基点 revision，同时以最新当前指针作为 expected_revision。

全尺寸 export 只读取完整检查点。预览请求改变尺寸、区域或 filter 会形成新缓存规格，任何缩略缓存都不会用于导出或续编恢复。输出路径必须在当前工程之外，覆盖和 PNG/JPEG 行为遵守既有 codec 契约。

## 修改旧步骤参数

```sh
pic-cli project checkpoint work.pic --revision r17 --json
pic-cli project revise work.pic --step-revision r18 --params '{"exposure":0.7,"brightness":0,"contrast":1,"saturation":1}' --expect-revision r20 --json
```

step-revision 是要修改的那一步输出 revision，必须属于当前指针的祖先路径，不能把数字当位置。params 是同一操作类型的完整参数对象，不作部分合并。程序先检查 expected_revision，验证替换及后续操作，恢复该步骤的输入，执行替换步骤和全部旧后缀，再通过原有提交路径发布为一个新撤销组。执行/预算/发布失败不改变原 manifest；旧 revision 和 ops 文件始终不变。后缀长度仍受单次 max_operations 和总历史预算限制。

20 步历史在已有 r17 检查点时修改第 18 步，只执行新 3 步；有 r2 检查点时修改第 3 步，重算新 18 步。改变几何参数导致后续 crop 无效时整个修改失败。旧状态可继续指定 revision 导出，undo 回到新组基点，redo 沿新路径恢复。

## 操作模板换图

```sh
pic-cli project template-export work.pic --revision r2 --output recipe.json --json
pic-cli project template-run --template recipe.json --bindings bindings.json --output another.pic --json
pic-cli project export another.pic --output another.png --json
```

template-export 从选中 revision 的祖先路径生成 schema_version=1 的线性配方，包含 pixel_semantics、input_slot=input、target_slots=[canvas]，以及每步 op、op_version、target_slot、params_slot=step_N 和 suggested_params。它不携带源路径/摘要、revision、op_id、缓存或结果资产。suggested_params 仅供调用方参考，运行不会用它补齐缺失绑定。

假设配方第 1 步 crop，第 2 步 adjust，对应 bindings.json：

```json
{
  "schema_version":1,
  "inputs":{"input":"new-photo.png"},
  "targets":{"canvas":"canvas"},
  "params":{
    "step_1":{"x":20,"y":10,"width":400,"height":300},
    "step_2":{"exposure":0.5,"brightness":0,"contrast":1,"saturation":1}
  }
}
```

新输入、目标及**每一步完整参数**全部必填，包括 crop 坐标、resize 尺寸、filter 和空参数对象。未知、多余或缺失的绑定、旧 revision 字段均拒绝；不能只替换输入然后默认沿用旧坐标。输入路径相对于 bindings 文件的规范化父目录，模板路径和输出路径相对于 cwd。无需原工程存在。

当前配方仅支持无外部素材的直接单画布操作，源图通过 input 槽重新绑定。新增图层／蒙版／选区／composite，以及非 canvas 像素操作尚无模板绑定规则，导入导出以 unsupported_template 明确拒绝；未知操作/参数仍按原错误拒绝，后续支持须扩展显式素材/坐标绑定契约，不能静默复制旧内容输出。本轮不执行任何模型。

运行总是创建一个不存在的新 .pic 工程，在临时目录调用同一 create/apply 核心，完整成功后一次发布。几何无效等失败不留下半个目的工程；不能覆盖现有工程。结果 revision 是新项目内的编号，可能同样叫 r1/r2，但身份应按 (project, revision) 解释，没有继承旧项目的历史或缓存。空配方只创建 r0。

## 验收与计时

新增测试在 `crates/pic-core/src/project/tests/replay.rs`、`project/cache.rs` 和 `crates/pic-cli/tests/project/replay.rs`。CLI 测试每次调用启动独立进程，素材在临时目录生成。

| 条件 | 证据 |
| --- | --- |
| 精确恢复/全重放/直接管线一致，损坏回退 | checkpoints_match_direct_float_state_and_corruption_falls_back_to_earlier_prefix；逐 f32 位比较，含 HDR/负值/alpha/插值，覆盖截断/摘要/版本错误 |
| signed zero/subnormal/隐藏 RGB 精度 | snapshot_codec_preserves_signed_zero_subnormals_hdr_and_hidden_rgb_bits；另拒绝 checksum 有效但 alpha 非法的快照 |
| 第 18/3 步修改及旧历史稳定 | twenty_step_revise_reuses_prefix_and_never_changes_old_revisions；同场景跨进程测试复用 17/2 步、重算 3/18 步，核对原文件与输出 |
| 预览坐标、规格失效、stale 及完整导出 | preview_region_mappings_cache_variants_and_full_export_stay_independent；CLI 覆盖每个步骤、非法目标/区域/尺寸 |
| P0 跨进程完整闭环 | p0_checkpoint_intermediate_preview_continue_export_and_clear_replay_in_new_processes；导入→裁剪/调色→检查点→中间观察→续编→导出→删源文件/清缓存→重放比对 |
| 预算、损坏、资产变化与安全路径 | cache_budgets_bound_both_tiers_and_never_evict_required_assets；cached_pixels_never_hide_missing_changed_assets_or_changed_content_identity；unsafe_cache_paths_and_failed_snapshot_writes_fall_back_without_touching_authority |
| 零字节清理、并发预算 | clear_removes_zero_length_and_truncated_managed_files_only；concurrent_checkpoint_publishers_share_disk_budget_and_clear_zero_length_entries；核对 assets/ops/manifest 保留 |
| 显式模板绑定、新历史与失败原子性 | templates_require_explicit_rebinding_and_create_atomic_independent_history；删除原工程后换图，与直接管线核对输出，缺绑定/旧 revision/内容结果拒绝 |

沿用既有毫秒字段：缓存键/路径选择及记录构造的未单独计时开销进入 total_ms；源资产验证、快照读入与解包计入 read_ms；需要时的源图解码计入 decode_ms；重放、新操作、完整 Document 合成、图层／蒙版渲染及预览区域/采样计入 process_ms；最终输出编码计入 encode_ms；快照序列化、缓存锁等待/淘汰/发布与普通输出发布计入 write_ms。内存命中无需快照 I/O。库调用累加阶段时间，CLI 设置 total_ms；本任务验收正确性和重算步数，没有宣称大图性能门槛。

本任务完成时，README 中的 fmt、clippy、workspace tests（90 项）、release build、git diff check 全部通过；另以 release 二进制运行 cli 与 project 集成测试（40 项）通过。Linux/POSIX 存储、无 fsync 保证及 codec 输入子集边界保持不变。
