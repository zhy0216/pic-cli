# 功能与兼容性矩阵

当前版本覆盖任务 01 基础核心、任务 02 几何/编解码与任务 03 调色/滤镜。`capabilities --json` 的 `status` 只有 `supported`（已支持）、`partial`（明确子集）、`not_implemented`（未实现）；`scope` 区分当前实现 `current`、后续直接编辑任务 `planned`、本轮范围外的未来项 `roadmap`。未来项不会出现在可执行 `operations` 清单里。

## 三类功能目标

| 类别 | 目标内容 | 当前状态 | 本轮范围 |
| --- | --- | --- | --- |
| 照片/普通像素编辑 | 图片信息、PNG/JPEG 读写、有序管线、几何、调色、滤镜 | identity/几何/调色/滤镜已支持；格式和管线为 partial | 当前参数和像素语义已固定，后续按测量优化 |
| 图层与合成 | 图层、位置/变换、混合模式、蒙版/选区、分组、文字、调整层 | not_implemented | 后续直接编辑任务继续实现 |
| 智能编辑 | 智能抠图、主体分割、智能修复、生成填充/扩图 | not_implemented，roadmap | 仅未来可做，不选型、不接模型、不要求 LLM 参与 |

源素材 + 不可变 ops 的工程历史、跨进程续编、检查点、逻辑步骤预览服务于前两类目标；本版只预留类型/契约，全部执行能力仍为 not_implemented。

## 当前精确能力

| 能力 | 状态 | 实际可用范围 |
| --- | --- | --- |
| help / version / capabilities | supported | 文本或稳定 JSON，包括解析错误 |
| info | supported | 对受支持输入完整解码并读取尺寸、通道、透明度及色彩假设 |
| identity | supported | v1，target=canvas，params={}；单命令与管线共用核心，工作像素不变 |
| run / pipeline | partial | schema_version=1，有序 identity/几何/调色/滤镜操作；每步以当前画布求值，保留逻辑索引和显式参数，未知操作拒绝 |
| PNG codec | partial | 静态 8-bit 灰度/RGB/RGBA 输入、RGBA8 输出，压缩 0–9，默认 6；完整准入限制见执行契约 |
| JPEG codec | partial | 8-bit 灰度/RGB/YCbCr 输入、RGB 输出；quality 1–100，默认 90；透明输入需显式不透明背景 |
| EXIF 主图方向 | supported | PNG/JPEG 方向 1–8 归一化后查询/编辑；无效或重复 EXIF 拒绝 |
| 色彩管理 / 动画 / 高位深 | not_implemented | ICC、非 sRGB EXIF 色彩、独立 gamma/色度/HDR、动画与高位深拒绝 |
| crop / resize / rotate / flip / canvas | supported | 单命令与 JSON 共用核心；nearest/bilinear、顺时针 ±360°、扩展/固定旋转画布、九个画布锚点；详见 [参数契约](geometry-codecs.md) |
| adjust / levels / curves / grayscale / invert / blur / sharpen | supported | EV、亮度、对比度、饱和度、色阶、有序折线曲线、灰度、反相、高斯模糊及 unsharp；线性 RGBA32F，CLI/JSON 同核心，详见 [调色与滤镜契约](adjustments-filters.md) |
| 图层、蒙版、分组、文字、调整层 | not_implemented | 仅后续目标，无假图层状态 |
| project / revision / replay / checkpoint / preview | not_implemented | 仅 document 类型和未来提交契约，无持久化格式读写承诺 |
| 智能能力 | not_implemented / roadmap | 无后端、模型依赖、凭据或网络请求 |

## 与 Photoshop 等工具的对齐维度

| 维度 | 当前可验收事实 | 没有承诺的事项 |
| --- | --- | --- |
| 功能覆盖 | 信息查询、有序几何/调色/滤镜管线和真实 codec 子集 | 任何尚未实现的像素算法、图层行为或智能效果 |
| 参数语义 | JSON/CLI 同一核心；几何采样、锚点、编码参数、EV/倍率、色阶/曲线与滤镜公式、通道和 alpha 行为明确 | Photoshop 滑杆值、默认值或专有算法一一对应 |
| 输出质量 | PNG 接受范围内空管线解码 RGBA 精确；JPEG 固定小样本做误差检查 | 任意 JPEG 无损重编码、调色/生成效果与 Photoshop 像素一致 |
| 文件保真 | 标准 PNG/JPEG 像素导出；归一化主图 EXIF 方向，拒绝未支持色彩与无效 EXIF | 元数据保留、PSD 往返、RAW、专业印刷色彩或高位深输入 |
| 执行时延 | 本机新进程启动和小图 codec 的 30 次热文件缓存基线 | 比 Photoshop 或其他 CLI 更快、照片级编辑或跨平台时延门槛 |

具体输入限制和未来检查点精度见 [执行契约](foundation-contract.md)，基础测试见 [任务 01 验收记录](foundation-validation.md)，几何/EXIF 与实际进程验证见 [任务 02 契约与验收](geometry-codecs.md)，调色/滤镜及混合管线验证见 [任务 03 契约与验收](adjustments-filters.md)。
