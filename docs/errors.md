# JSON、退出码与错误恢复

agent 应传 `--json` 并检查退出码和 `ok`。stdout 只输出一行 schema-v1 envelope；成功 `data` 非 null、`error=null`，失败 `data=null`、`error={code,message}`。按稳定的 `error.code` 分支，message 用于诊断，不要解析其中的英文。警告不代表失败，位于 `warnings` 数组；常见 `assumed_srgb`、`metadata_not_preserved`、`alpha_flattened`、`cache_write_skipped`。

| 退出码 | 含义 | stdout / stderr |
| --- | --- | --- |
| 0 | 成功，包括 help/version | `--json` 时单个 JSON，stderr 为空 |
| 1 | 解析之后的执行/参数/文件/工程错误 | `--json` 时失败 JSON，stderr 通常为空 |
| 2 | 命令行缺参数、未知选项、clap 值解析失败 | `--json` 时失败 JSON，command=`cli`，code=`invalid_argument`，stderr 为空 |

不带 `--json` 时数据/错误仍是缩进 JSON，错误另写 stderr；help/version 是文本。stdout 写入/flush 失败是例外：可能没有完整 JSON，并在 stderr 输出诊断、返回 1；输出文件或工程可能已经发布。调用方应保留 stdout/stderr，重查输出和工程状态，再决定是否重试。不要把任何非零退出都当作“肯定没提交”。

本表覆盖当前 `ErrorCode`；[包验收脚本](../scripts/verify-package.py) 实际检查缺参数、缺图片/字体、无效 JSON/版本/操作、目标已存在、透明 JPEG、revision 冲突、redo 边界、模板缺绑定/不支持、缺字和内嵌资产缺失/损坏，并检查失败不改变 manifest/已有输出。

| code | 原因及恢复方式 |
| --- | --- |
| invalid_argument | 缺必需 JSON 字段、数值/范围非法、尺寸不适用、空 apply、模板缺绑定等。结合该命令 help 和 capabilities 补全/修正；同一 code 可能是退出 1 或 2 |
| invalid_json | JSON 语法或非有限/溢出数值非法。用 JSON 解析器生成文件，避免拼接 shell 字符串 |
| unsupported_version | 不支持的 schema/op 版本。使用匹配版本的二进制或显式转换；不要直接改版本号欺骗重放 |
| unknown_operation | 不在 operations 列表内，包括智能操作。先查 capabilities，换成已支持的直接编辑 |
| invalid_target | 稳定 ID 不存在或目标类型不适用。inspect 当前版本，选择正确图层；多层 canvas 调色需指定 raster 层 |
| invalid_mask | 蒙版尺寸/灰度不合法。提供与目标本地尺寸相同的 8-bit 灰度 coverage，RGB 各通道须相等 |
| invalid_hierarchy / dependency_cycle | 非法 parent、跨组/前向 clip、有依赖时删除、依赖环。先检查树和同级顺序，逐步解除或调整依赖 |
| invalid_font | 字体损坏或类型不支持。绑定静态单 face TrueType glyf 字体；没有自动 fallback |
| missing_glyph / unsupported_text | 字体缺指定码点或脚本/控制字符/布局不支持。选择包含所需字形的受支持字体和文本；换字体不能启用尚未实现的 CJK/bidi |
| unsupported_template | 模板包含图层、字体、蒙版、选区、composite 或非 canvas 操作。改用显式绑定资源的 run/apply 管线；当前模板只支持单画布子集 |
| file_not_found | 外部输入/管线/字体文件缺失。检查 cwd 或 JSON 文件父目录对应路径 |
| io_error | 读取、权限、磁盘空间、锁或发布错误。修复具体 I/O 原因后 inspect/检查目标再重试 |
| unsupported_format | 非 PNG/JPEG、调色板/高位深等未支持编码。调用方先用合适工具显式转换为可接受输入 |
| unsupported_color / unsupported_metadata | ICC、非 sRGB 色彩声明、独立 gamma/HDR、畸形/重复 EXIF 等。经可信转换生成 8-bit sRGB 输入，不要只删 ICC 假装完成转换 |
| decode_failed / encode_failed | 文件损坏或 codec 失败。检查输入完整性、输出参数和可接受格式；保留源文件 |
| output_exists | 目标已存在。使用新路径，或对普通导出明确 `--overwrite`；工程 create/template-run 不支持覆盖 |
| alpha_not_supported | 透明图导出 JPEG 未提供背景。用 PNG 或显式 `--jpeg-background '#ffffff'` |
| resource_limit | 尺寸、缓冲、输入文件、管线、历史或图层深度超准入。缩小工作量/图层/半径；cache 参数只控制派生缓存，不能增大核心像素限额 |
| invalid_project | manifest/ops 结构或历史不合法。恢复可信工程备份；不要自行修补 ops/指针 |
| asset_missing / integrity_mismatch | 工程内必需素材/字体缺失或摘要不符。恢复完全相同字节的资产或完整备份；检查点不替代资产，清缓存不能修复缺图 |
| unsafe_path | 工程内部路径、符号链接或输出位置不安全。使用普通自包含目录，导出到工程外；不要绕开路径检查 |
| revision_not_found | 请求不在该工程的已提交历史中。inspect 后重新选择，revision 只在其工程内有意义 |
| revision_conflict | expected revision 与当前指针不匹配。重新 inspect/preview/分析，再提交；不能只更新 token 后盲重试旧坐标 |
| history_boundary | 已到 undo/redo 边界或 redo 路径被新编辑清除。读取 group_boundaries/active_revisions；旧记录仍可按 revision 只读导出 |

`project inspect --revision OLD` 返回所选 `revision` 和真实 `current_revision`。从旧步骤继续时使用 `--revision OLD --expect-revision CURRENT`；两个参数含义不同。查询和预览不追加 ops。一次多步 apply 全部成功才发布一个撤销组，失败不发布半组。缓存损坏可以回退重放，`cache_write_skipped` 和 `disk_stored=false` 表示没有成功保存该加速副本，不能记为缓存命中。

准确的存储、锁、发布和计时边界见 [工程契约](project-history.md) 与 [执行契约](foundation-contract.md)。`expected_revision` 检查当前指针，不提供独立单调事务编号；工程没有断电 fsync 保证。
