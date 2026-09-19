# 功能与兼容性矩阵

当前版本为任务 01 的基础骨架。`capabilities --json` 的 `status` 只有 `supported`（已支持）、`partial`（明确子集）、`not_implemented`（未实现）；`scope` 区分当前实现 `current`、后续直接编辑任务 `planned`、本轮范围外的未来项 `roadmap`。未来项不会出现在可执行 `operations` 清单里。

## 三类功能目标

| 类别 | 目标内容 | 当前状态 | 本轮范围 |
| --- | --- | --- | --- |
| 照片/普通像素编辑 | 图片信息、PNG/JPEG 读写、有序管线、几何、调色、滤镜 | 基础读写/identity 已支持；格式和管线为 partial；几何、曝光/饱和度、曲线和滤镜 not_implemented | 后续直接编辑任务继续实现 |
| 图层与合成 | 图层、位置/变换、混合模式、蒙版/选区、分组、文字、调整层 | not_implemented | 后续直接编辑任务继续实现 |
| 智能编辑 | 智能抠图、主体分割、智能修复、生成填充/扩图 | not_implemented，roadmap | 仅未来可做，不选型、不接模型、不要求 LLM 参与 |

源素材 + 不可变 ops 的工程历史、跨进程续编、检查点、逻辑步骤预览服务于前两类目标；本版只预留类型/契约，全部执行能力仍为 not_implemented。

## 当前精确能力

| 能力 | 状态 | 实际可用范围 |
| --- | --- | --- |
| help / version / capabilities | supported | 文本或稳定 JSON，包括解析错误 |
| info | supported | 对受支持输入完整解码并读取尺寸、通道、透明度及色彩假设 |
| identity | supported | v1，target=canvas，params={}；单命令与管线共用核心，工作像素不变 |
| run / pipeline | partial | schema_version=1，空数组或有序 identity；保留逻辑索引，未知操作拒绝 |
| PNG codec | partial | 静态 8-bit 灰度/RGB/RGBA 输入、RGBA8 输出；完整准入限制见执行契约 |
| JPEG codec | partial | 8-bit 灰度/RGB/YCbCr 输入、无透明 RGB 输出；quality 1–100，默认 90 |
| 色彩管理 / EXIF / 动画 / 高位深 | not_implemented | 目前相关表示/元数据拒绝，不静默转换或丢失方向 |
| crop / resize / rotate / flip / canvas | not_implemented | 无命令壳、无成功占位实现 |
| adjust / levels / curves / blur / sharpen | not_implemented | EV/倍率等只在文档约定，尚不可执行 |
| 图层、蒙版、分组、文字、调整层 | not_implemented | 仅后续目标，无假图层状态 |
| project / revision / replay / checkpoint / preview | not_implemented | 仅 document 类型和未来提交契约，无持久化格式读写承诺 |
| 智能能力 | not_implemented / roadmap | 无后端、模型依赖、凭据或网络请求 |

## 与 Photoshop 等工具的对齐维度

| 维度 | 当前可验收事实 | 没有承诺的事项 |
| --- | --- | --- |
| 功能覆盖 | 信息查询、空/identity 管线和真实 codec 子集 | 任何尚未实现的像素算法、图层行为或智能效果 |
| 参数语义 | JSON/CLI 同一核心；JPEG quality 明确；后续 EV/倍率有单位契约 | Photoshop 滑杆值、默认值或专有算法一一对应 |
| 输出质量 | PNG 接受范围内空管线解码 RGBA 精确；JPEG 固定小样本做误差检查 | 任意 JPEG 无损重编码、调色/生成效果与 Photoshop 像素一致 |
| 文件保真 | 标准 PNG/JPEG 像素导出；拒绝未支持色彩/方向内容 | 元数据保留、PSD 往返、RAW、专业印刷色彩或高位深输入 |
| 执行时延 | 本机新进程启动和小图 codec 的 30 次热文件缓存基线 | 比 Photoshop 或其他 CLI 更快、照片级编辑或跨平台时延门槛 |

具体输入限制和未来检查点精度见 [执行契约](foundation-contract.md)，实际测试见 [验收记录](foundation-validation.md)。
