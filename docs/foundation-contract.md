# 基础执行与版本契约

本文件定义任务 01 建立、任务 02 扩展的基础边界与后续实现需要遵守的接口。几何与编解码的详细参数见 [任务 02 契约](geometry-codecs.md)。预留工程契约不代表已具备工程功能。智能能力处于未来 roadmap，本轮没有模型选择、模型后端或 LLM 调用。

## 输入、输出与色彩

| 项目 | 当前行为 |
| --- | --- |
| 输入识别 | 通过文件内容识别 PNG/JPEG，不信任扩展名；仅接受普通文件，支持指向普通文件的输入符号链接 |
| PNG | 静态 8-bit grayscale、gray-alpha、RGB、RGBA；支持交错；保留透明度与 alpha=0 时的隐藏 RGB |
| PNG 限制 | 调色板及 1/2/4/16-bit、APNG 拒绝；`iCCP/gAMA/cHRM/cICP/mDCV/cLLI` 拒绝，即使同时带 sRGB 标记；当前不做颜色管理 |
| JPEG | 8-bit grayscale、RGB/YCbCr 的 baseline/extended-sequential/progressive 解码；CMYK/YCCK、其他编码模式拒绝 |
| sRGB | PNG sRGB 标记或 EXIF ColorSpace=1 接受；无色彩标记的 PNG/JPEG 要求调用方提供 sRGB 数据，返回 `assumed_srgb` 警告；无法证明无标记图片原本属于 sRGB |
| ICC | PNG/JPEG 的 ICC 输入全部拒绝，包括嵌入 sRGB ICC；不静默丢弃后继续处理 |
| EXIF | JPEG EXIF / PNG eXIf 的主图方向 1–8 在操作前归一化；支持大小端 classic TIFF，缺省方向为 1，重复或无效方向拒绝。EXIF 声明非 sRGB/未校准色彩或独立 gamma/ICC/色度信息时拒绝；其他元数据导出时不保留 |
| `info` | 使用与 `run` 相同的准入和完整解码，返回归一化后的尺寸、原始存储尺寸、EXIF 方向、通道、透明度和色彩假设；不是任意文件的元数据探针 |
| PNG 输出 | RGBA8，`--png-compression 0..9`，默认 6；0 不压缩，1–9 是 deflate level；自适应行滤波。不保留原始元数据 |
| JPEG 输出 | RGB8，quality 整数 1–100，默认 90；有任何 alpha < 1 默认拒绝，显式 `--jpeg-background` 提供不透明 sRGB 背景后按线性光铺底；格式不匹配的编码参数拒绝 |
| 文件保真 | 输出不保留 EXIF、文本、ICC 等源元数据；`metadata_not_preserved` 警告明确说明。JPEG 再编码有损，不承诺压缩字节或像素相同 |

元数据准入扫描只决定可接受的输入子集，实际编解码全部使用 `image`。后续扩大范围时，需要增加相应颜色/方向测试并更新能力声明。

## 工作像素与精度

`document::Raster` 存储行优先 `[f32; 4]` RGBA，每像素 16 字节；语义标识固定为 `linear_srgb_rgba32f_straight_v1`。RGB 使用线性光 sRGB，alpha 为 straight/unassociated（未预乘）的覆盖率。导入时 sRGB 的 8-bit RGB 经标准分段传递函数转为线性值，alpha 除以 255；灰度复制到 RGB。

RGB 可保留有限的负值和大于 1 的值，不在步骤间裁切或量化。alpha 必须有限且在 `[0,1]`；NaN/Inf 拒绝。`Raster` 的像素不可从公共接口直接修改，克隆共享 `Arc<Vec<[f32;4]>>`；操作构造新的有效 Raster 时须继续检查尺寸与资源限制。identity 和空管线保持全部 f32 位模式及共享存储。任务 02 的 bilinear 缩放/旋转在临时预乘表示下采样，然后转回 straight alpha；RGB 不在步骤间裁切。裁剪、翻转、整数正交旋转和 nearest 缩放精确保留被复制样本的位模式。调色/合成尚未实现，后续也必须显式遵守 alpha 约定。

只在最终输出时将 RGB 裁切到 `[0,1]`、转换回非线性 sRGB 并四舍五入为 8-bit；alpha 同样四舍五入。覆盖全部 256 级通道值的 PNG 往返测试要求解码 RGBA 完全相同，含透明像素的隐藏 RGB。

未来检查点必须保留原始 f32 位模式、通道顺序、宽高、色彩语义及 alpha 模式，例如有版本头的 little-endian IEEE-754 样本资产。不能用 RGBA8 PNG、JPEG、缩略图替换工作像素，也不能仅保存扁平预览来恢复未来多图层状态。本任务没有实现检查点序列化、恢复或缓存。

## 操作、顺序与坐标

`OperationSpec` 是请求规格，`validate` 得到可执行 `Operation`。支持 `identity/crop/resize/rotate/flip/canvas` v1；目标为 `canvas`，identity 参数为 `{}`。JSON 的版本、目标、采样和背景参数显式必填，resize 的宽高至少一个非 null；CLI 的默认值会补齐到返回的 `steps[].params`，未来 ops 可复用同一规格。未支持的操作、参数、版本、目标和未知字段都拒绝，不退化为空操作。

`PipelineSpec` 固定为 `{"schema_version":1,"operations":[...]}`。整个管线先解析和校验，再读取图片；数组顺序就是执行顺序。`Pipeline::execute` 对每一步调用 `Operation::apply_with_limits`，返回从 0 开始的逻辑步骤索引。所有单操作 CLI 用 `Pipeline::single` 创建一步管线，然后与 `run` 一起进入 `pipeline::run`。未进行步骤融合或重排；单次运行解码一次，结束编码一次，不生成中间有损图像。

管线内未来的素材路径统一通过 `ResourceResolver` 解析：相对路径基于**规范化后管线文件的父目录**，而不是进程 cwd；管线符号链接以链接目标文件的目录为基准。绝对路径不变，允许 `..`，没有路径沙箱。`from_json` / `single` 的库调用显式提供资源目录；CLI 单操作默认 cwd。目前 identity/几何操作没有外部素材参数，该解析器已实现并测试，未来操作不得自行另定规则。

CLI 的 `--input`、`--output`、`--pipeline` 路径都相对于 cwd。结果返回规范化输入、输出父目录和资源根目录；路径要求有效 UTF-8，避免发布成功后无法准确序列化路径。

几何操作的统一坐标契约：原点在当前画布左上角，x 向右、y 向下，单位是像素；像素中心为 `(x+0.5,y+0.5)`，矩形采用左上含、右下不含的 `[x,x+w) × [y,y+h)`，宽高正整数。每步坐标基于前一步结果，不依赖最初画布尺寸。输入首先应用 EXIF 方向。图层局部坐标必须显式转换；当前几何只支持 canvas。

参数语义约定（rotation 已实现，其他行留给后续任务）：

| 参数 | 单位/中性值 | 后续实现约束 |
| --- | --- | --- |
| exposure | EV，0 不变 | 在线性 RGB 上乘 `2^EV`，有限值；确切可接受范围由调色任务固定并测试 |
| saturation | 倍率，1 不变、0 为灰度 | 非负有限值；颜色模型、灰度权重和上限由调色任务明确，不沿用 Photoshop 滑杆百分数 |
| opacity | 覆盖率 `[0,1]`，1 不变 | 不使用 0–100 的模糊单位；需与像素 alpha 明确组合 |
| rotation | 度，顺时针为正，`[-360,360]` | 画布中心旋转，nearest/bilinear，默认扩展边界；详见 [几何契约](geometry-codecs.md) |
| blur | 像素单位 sigma | 算法、边界采样与允许范围待后续任务固定 |
| brightness/contrast/curves | 尚未定算法 | 不承诺 Photoshop 参数或输出完全等价 |

## 资源准入

`ResourceLimits::default()` 同时用于 CLI 与能力输出，库调用可显式传入更小/不同限额；本版没有资源限额 CLI 参数。

| 限制 | 默认值 |
| --- | --- |
| 单边尺寸 | 32,768 像素 |
| 像素数 | 40,000,000 |
| 压缩输入 | 128 MiB |
| 估算工作缓冲 | 1 GiB，以 `像素数 × 32` 作准入估算，覆盖 RGBA32F 及输入/输出样本余量 |
| 管线文件 | 1 MiB |
| 管线操作数 | 10,000 |

所有限制同时满足，因此默认缓冲预算实际会先将像素数限制到 33,554,432。读文件有长度上限，尺寸和乘法在分配工作像素之前检查，解码器收到相同尺寸及分配限制，工作/导出样本使用可失败的预分配。每个中间画布重新检查尺寸，几何另检查同时存在的源/目标及缩放临时缓冲；极端宽高比的缩放可能因临时缓冲超限而拒绝，见几何契约。压缩输入、codec scratch、输出编码缓冲和运行时开销不构成严格的全进程 RSS 上限；操作系统 OOM、磁盘空间和运行超时仍由宿主控制。本版不设置模型、GPU 或线程池；单个处理管线按步骤串行执行。

## 发布、错误与结果

输出格式取扩展名或显式 `--format`（显式格式优先，即使后缀不同）。目标父目录必须已存在。先检查覆盖参数，再完成处理/编码；在目标同目录创建唯一 `.pic-*` 临时文件，写完并 flush 后发布。默认使用 `persist_noclobber` 保证并发创建时不覆盖赢家；`--overwrite` 使用替换发布，拒绝已有目录/符号链接。允许输入输出相同，但必须显式覆盖，且完整读入/解码后才替换。

失败不发布新的成功目标，已有目标保持原样，普通错误会清除临时文件。替换可能改变旧文件的权限；新文件使用 tempfile 默认私有权限。不执行 `fsync`，不承诺突然断电后的持久性；进程被强制终止可能遗留隐藏临时文件。原子发布语义已在本机 Linux/ext4 验证，其他平台/文件系统需补充验证。stdout 失败可能发生在成功文件发布之后，CLI 返回非零并将诊断写 stderr，此时调用方应检查已报告的目标路径或重查输出。

所有 `--json` 请求输出同一 envelope，stdout 一行一个完整对象，没有进度日志、ANSI 前缀或文本帮助混入：

```json
{
  "schema_version": 1,
  "engine_version": "0.1.0",
  "command": "run",
  "ok": true,
  "data": {},
  "error": null,
  "warnings": [],
  "timings": {
    "validation_ms": 0.0,
    "read_ms": 0.0,
    "decode_ms": 0.0,
    "process_ms": 0.0,
    "encode_ms": 0.0,
    "write_ms": 0.0,
    "total_ms": 0.0
  }
}
```

上例省略具体 `data` 内容，耗时仅示意。成功的 `error=null`，失败的 `data=null` 且 `error={"code":"...","message":"..."}`。调用方按 code 分支，message 不作为稳定解析接口。代码包括 `invalid_argument`、`invalid_json`、`unsupported_version`、`unknown_operation`、`invalid_target`、`file_not_found`、`io_error`、`unsupported_format`、`unsupported_color`、`unsupported_metadata`、`decode_failed`、`encode_failed`、`output_exists`、`alpha_not_supported`、`resource_limit`。退出码为 0/1/2（成功/执行错误/CLI 解析错误）。

耗时是单调时钟的毫秒浮点数，失败阶段也计时；未运行的阶段为 0。validation 包含管线读取/解析和输出校验；read 只含图像读取；decode 包含准入和转换为工作像素；process 包含有序操作；encode 包含导出量化和压缩；write 包含创建临时文件、写入、flush、发布、关闭。total 从 main 入口开始，到结果序列化前结束，含 CLI 解析和未分类开销；不包含进程启动、stdout 序列化/写入及退出，因此性能基准另测父进程端到端 wall time。阶段和不是进程总时延，库调用方负责设置 total。

## 后续工程契约（只预留，不实现）

`DOCUMENT_SCHEMA_VERSION=1`、`RevisionId` 和 `TargetId` 目前只是类型/契约，不能据此创建或打开工程。未来模块需要区分：

| 标识 | 责任 |
| --- | --- |
| result / pipeline / document 的 schema_version | 各自结构的版本，独立升级；未知版本拒绝或显式迁移 |
| revision | 不可变的逻辑状态 ID，不能等同数组下标、文件时间或 schema_version |
| op_id / op_version | 唯一已提交操作 ID 与该操作的语义版本；保存补齐默认值的实际参数，不用新默认值重放旧记录 |
| stable target | 不随图层显示名、排序或重命名变化的目标 ID；stateless 单图目前使用 `canvas` |
| engine / pixel semantics | 重放和缓存身份包含算法版本、`PIXEL_SEMANTICS`、色彩/采样设置及依赖资产内容 |

源素材、不可变 ops 及必要结果资产是权威状态。提交记录包含输入/输出 revision、稳定目标、明确参数和资产引用；失败请求、查询与预览不进入编辑序列。检查点和预览属于可丢弃的派生数据，不能成为另一份可独立修改的权威文档。跨进程恢复必须只依赖持久化素材和记录。

未来一组提交先校验并执行、写资产/不可变记录，最后原子发布 manifest 并比较 expected_revision；并发冲突不得相互覆盖。每个组内逻辑步骤都保留观察边界，任意步骤预览要返回 revision、步骤/目标 ID、画布/裁剪/预览尺寸及坐标映射。从旧步骤继续产生新记录，不改写原记录。预览、导出及重放沿用相同 operation/pipeline 像素语义；本任务未提前实现历史、回放执行器、撤销、检查点或预览命令。

实现参考：本地 gimpish 的结构/稳定图层 ID、AgentBrush 的统一结果、Compositor 的不可变像素共享。实际后端行为以锁定的 [image ImageDecoder 接口](https://docs.rs/image/0.25.10/image/trait.ImageDecoder.html) 和 [tempfile 发布接口](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html)及本仓库测试为准，未复制上游指令或采用第二套图像后端。
