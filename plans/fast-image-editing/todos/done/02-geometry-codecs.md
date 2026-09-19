difficulty: hard
agent: inherit

# 基础几何与图像编解码语义

## T1 · 实现几何操作并固定真实文件行为

前置依赖：01-foundation.md。

要做什么：在已建立的 operation/pipeline/codec 与 CLI 中加入裁剪、缩放、旋转、水平/垂直翻转、画布大小及 PNG/JPEG 输出参数。旋转角度和插值范围显式声明，支持基本直角和有文档的任意角旋转；尺寸、越界、背景色、锚点和透明 JPEG 处理行为要明确。以 fast_image_resize 评估/实现缩放，正确处理 alpha 与既定工作色彩。处理 EXIF 方向，明确 ICC/不支持色彩与位深的拒绝或转换，不静默丢信息。管线按当前画布顺序执行，单命令与 JSON 等价。

预计修改：实际 pic-core 的 codec、operation/pipeline 几何模块，pic-cli 命令定义，相关 docs 与测试；本 todo 和状态。沿用 01 的布局，不创建第二个像素核心。

验收条件：

- [x] 所有列出的几何操作通过 CLI 和 JSON 管线执行；参数校验、坐标、插值、输出质量/压缩参数有文档和能力描述。
- [x] 使用可解析小图检验坐标、操作顺序、旋转/翻转、裁剪与画布背景；PNG alpha、透明边缘缩放不出现黑边。
- [x] JPEG EXIF 方向有真实编码样例回归；PNG/JPEG 往返、透明 JPEG 策略和不支持格式/颜色行为有测试。
- [x] 失败不覆盖已有输出；溢出、非法尺寸和资源限制返回结构化错误。
- [x] 全部仓库校验通过。

验证：完整仓库校验；独立进程运行单命令与等价多步 JSON，对解码尺寸、坐标及像素作比较。


## 完成记录

2026-09-19，全部验收通过；完整参数、修改入口和逐条测试证据见 [几何与编解码契约](../../../../docs/geometry-codecs.md)。

- CLI/JSON：五种几何操作及双轴翻转等价，返回显式参数；六步管线与逐条独立进程逐像素、坐标一致。
- 精度/alpha：3×2 HDR/负零/隐藏色置换、九锚点、任意角旋转、一维/二维线性预乘缩放及 PNG 透明往返通过。
- Codec：真实 JPEG 八方向 × 大小端（含扫描后 APP1）、PNG EXIF、JPEG/PNG 往返、压缩级别、JPEG 铺底与格式/色彩拒绝通过。
- 失败：越界/溢出/非法尺寸/中间画布及采样 scratch 限制返回结构化错误；已有目标字节保持不变。
- 校验：fmt check、clippy `-D warnings`、workspace test（40 个）、release build、git diff check 全通过；实际 release CLI 的 21 个独立进程回归全通过。

无 blocker。限于 8-bit sRGB PNG/JPEG 和 nearest/bilinear；不做 ICC 转换，EXIF 仅检查主图方向与相关色彩，其他元数据导出时不保留。只验证本机 Linux，内存为准入预算。既有 ops/expected_revision 契约不变。其他 todo 状态和 plan.md 保留，等待协调器集成。
