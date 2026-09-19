# 功能与兼容性矩阵

当前版本覆盖任务 01 基础核心、任务 02 几何/编解码、任务 03 调色/滤镜、任务 04 工程历史、任务 05 重放/预览、任务 06 图层/蒙版及任务 07 分组/剪贴/文字/调整层，并已完成任务 11 实测优化与任务 12 打包验收。`capabilities --json` 的 `status` 只有 `supported`（已支持）、`partial`（明确子集）、`not_implemented`（未实现）；`scope` 区分当前实现 `current`、后续直接编辑任务 `planned`、本轮范围外的未来项 `roadmap`。未来项不会出现在可执行 `operations` 清单里。

## 三类功能目标

| 类别 | 目标内容 | 当前状态 | 本轮范围 |
| --- | --- | --- | --- |
| 照片/普通像素编辑 | 图片信息、PNG/JPEG 读写、有序管线、几何、调色、滤镜 | identity/几何/调色/滤镜已支持；格式和管线为 partial | 当前参数和像素语义已固定，后续按测量优化 |
| 图层与合成 | 图层、位置/变换、混合模式、蒙版/选区、分组、文字、调整层 | partial：图层／混合／变换／蒙版／选区／分组／剪贴／文字子集／点调整层已支持 | 直接参数编辑与完整工程恢复 |
| 智能编辑 | 智能抠图、主体分割、智能修复、生成填充/扩图 | not_implemented，roadmap | 仅未来可做，不选型、不接模型、不要求 LLM 参与 |

源素材 + 不可变 ops 的工程历史、跨进程续编、精确检查点、区域/缩放预览和显式绑定模板已实现，服务于前两类目标；当前状态支持完整独立图层，旧单画布历史直接兼容。

## 当前精确能力

| 能力 | 状态 | 实际可用范围 |
| --- | --- | --- |
| help / version / capabilities | supported | 文本或稳定 JSON，包括解析错误 |
| info | supported | 对受支持输入完整解码并读取尺寸、通道、透明度及色彩假设 |
| identity | supported | v1，target=canvas 或 raster 图层 ID，params={}；单命令与管线共用核心，工作像素不变 |
| run / pipeline | partial | schema_version=1，有序 identity/几何/调色/滤镜/图层/蒙版/组/文字/调整层操作；每步以当前画布求值，保留逻辑索引和显式参数，未知操作拒绝 |
| PNG codec | partial | 静态 8-bit 灰度/RGB/RGBA 输入、RGBA8 输出，压缩 0–9，默认 6；完整准入限制见执行契约 |
| JPEG codec | partial | 8-bit 灰度/RGB/YCbCr 输入、RGB 输出；quality 1–100，默认 90；透明输入需显式不透明背景 |
| EXIF 主图方向 | supported | PNG/JPEG 方向 1–8 归一化后查询/编辑；无效或重复 EXIF 拒绝 |
| 色彩管理 / 动画 / 高位深 | not_implemented | ICC、非 sRGB EXIF 色彩、独立 gamma/色度/HDR、动画与高位深拒绝 |
| crop / resize / rotate / flip / canvas | supported | 单命令与 JSON 共用核心；nearest/bilinear、顺时针 ±360°、扩展/固定旋转画布、九个画布锚点；详见 [参数契约](geometry-codecs.md) |
| adjust / levels / curves / grayscale / invert / blur / sharpen | supported | EV、亮度、对比度、饱和度、色阶、有序折线曲线、灰度、反相、高斯模糊及 unsharp；线性 RGBA32F，CLI/JSON 同核心，详见 [调色与滤镜契约](adjustments-filters.md) |
| 图层、合成、蒙版、选区 | partial | 稳定 ID、有序显隐、opacity、无损变换、normal/multiply/screen/overlay、外部编码值 coverage、明确空间矩形并集；同一核心渲染与持久化，详见 [图层契约](layers-masks.md) |
| 分组、剪贴蒙版、调整层 | partial | 隔离有界组、稳定 parent/clip 依赖与环校验、lower-sibling clip、同组下方前缀的非破坏点调整；无 pass-through/空间滤镜调整层，详见 [契约](groups-text-adjustments.md) |
| 文字排版 | partial | 显式内嵌静态 TrueType；Rustybuzz shaping；Latin/Greek/Cyrillic 每行单字母表+Common/Inherited，LF换行、left/center/right，缺字/缺字体报错；无中文/bidi/自动换行/字体回退，详见 [契约](groups-text-adjustments.md) |
| project / revision / replay / undo / redo | supported | 自包含素材、SHA-256 去重与完整性校验、不可变 ops、整组原子提交、expected_revision 冲突控制、任意已提交步骤完整重放与续编；Linux 验证，详见 [工程契约](project-history.md) |
| preview / checkpoint / revise / template | partial | 完整图层／蒙版／组／文字字体／clip／调整参数精确 RGBA32F 检查点与独立预览缓存、磁盘/内存预算、损坏回退、任意步骤/画布或图层或蒙版/区域/尺寸及仿射坐标映射、旧步骤参数修改与前缀复用；模板新输入/目标/完整参数必填，暂不支持外部蒙版等内容依赖操作。见 [重放与预览契约](replay-preview.md) |
| 智能能力 | not_implemented / roadmap | 无后端、模型依赖、凭据或网络请求 |

## 与 Photoshop 等工具的对齐维度

| 维度 | 当前可验收事实 | 没有承诺的事项 |
| --- | --- | --- |
| 功能覆盖 | 信息查询、有序几何/调色/滤镜、图层/蒙版/文字、工程历史及真实 codec 子集 | 任何尚未实现的像素算法、图层行为或智能效果 |
| 参数语义 | JSON/CLI 同一核心；几何采样、锚点、编码参数、EV/倍率、色阶/曲线与滤镜公式、通道和 alpha 行为明确 | Photoshop 滑杆值、默认值或专有算法一一对应 |
| 输出质量 | PNG 接受范围内空管线解码 RGBA 精确；JPEG 固定小样本做误差检查 | 任意 JPEG 无损重编码、调色/生成效果与 Photoshop 像素一致 |
| 文件保真 | 标准 PNG/JPEG 像素导出；归一化主图 EXIF 方向，拒绝未支持色彩与无效 EXIF | 元数据保留、PSD 往返、RAW、专业印刷色彩或高位深输入 |
| 执行时延 | 本机 16 场景前后各 30 次 release 采样，包含 1080p/4K、图层和工程；见 [性能报告](performance.md) | 比 Photoshop 或其他 CLI 更快、所有 4K 编辑低于 1 秒或跨平台时延门槛 |

具体输入限制和检查点精度约束见 [执行契约](foundation-contract.md)，基础测试见 [任务 01 验收记录](foundation-validation.md)，几何/EXIF 与实际进程验证见 [任务 02 契约与验收](geometry-codecs.md)，调色/滤镜及混合管线验证见 [任务 03 契约与验收](adjustments-filters.md)。

## 平台、模型配置和交付状态

已验证 Linux x86_64 / Ubuntu 24.04 / glibc 2.39 / Rust 1.98.1 的 release 包与仓库外临时 cwd；其他平台、Rust 声明下限 1.88 未验证，详见 [打包与安装](packaging.md)。`supported` 表示约定范围内可执行，不表示全平台或 Photoshop 完全兼容。

目前没有“需模型配置才能启用”的已实现能力，capabilities 没有第四种 `requires_configuration` 状态。智能抠图/主体分割、修复、生成填充/扩图仍明确列入 [未来需求](../plans/fast-image-editing/roadmap/README.md)，状态是未实现，不因缺少模型配置而隐藏，也不要求本轮用户配置模型。
