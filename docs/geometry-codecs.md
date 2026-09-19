# 几何与编解码契约（任务 02）

全部操作扩展既有 `OperationSpec → Operation → Pipeline → Raster` 核心，`op_version=1`；单画布目标为 `canvas`，当前亦可对 raster 图层 ID 编辑本地像素，限制见 [图层契约](layers-masks.md)。单命令和 `run` 共用解析后的操作及文件处理入口。本任务当时只添加几何能力；当前工程/检查点见 [工程历史](project-history.md) 与 [重放/预览](replay-preview.md)，仍以原始素材和不可变 ops 为权威。模型功能仅为未来规划。

## 命令和 JSON

所有图像命令使用 `--input` / `--output`，路径相对 cwd；`--json` 返回同一版本化 envelope。操作坐标以输入 EXIF 归一化后的画布为起点，之后每一步基于前一步结果。原点左上，x 向右、y 向下；像素中心 `(x+0.5,y+0.5)`，矩形左上含、右下不含。宽高为正整数，坐标为非负整数；JSON/CLI 超出 u32 的参数拒绝。

| 操作 | CLI 参数 | JSON `params` | 行为 |
| --- | --- | --- | --- |
| crop | `--x X --y Y --width W --height H` | `{"x":1,"y":0,"width":2,"height":2}` | 整个矩形必须在当前画布内；越界报错，不自动裁切或补边 |
| resize | `--width W`、`--height H` 至少一个；`--filter nearest\|bilinear`，默认 bilinear | `{"width":1600,"height":null,"filter":"bilinear"}` | 双尺寸精确拉伸；缺一个时按当前宽高比推导，四舍五入（恰好一半向上），最小 1；省略维度与 null 等价 |
| rotate | `--degrees D`，`--filter nearest\|bilinear`（默认 bilinear），`--keep-size`，`--background COLOR`（默认透明黑） | `{"degrees":30.0,"expand":true,"filter":"bilinear","background":[0,0,0,0]}` | 有限角度 `[-360,360]`，顺时针为正，绕画布中心；默认扩展，keep-size 对应 expand=false |
| flip | `--axis horizontal\|vertical` | `{"axis":"horizontal"}` | horizontal 反转 x，vertical 反转 y；尺寸不变 |
| canvas | `--width W --height H`，`--anchor A`（默认 center），`--background COLOR`（默认透明黑） | `{"width":1920,"height":1080,"anchor":"center","background":[0,0,0,0]}` | 不缩放，按锚点补边或裁切 |

CLI 色值为带 `#` 的 `#RRGGBB` 或 `#RRGGBBAA`，各通道是 **8-bit 非线性 sRGB / alpha**；无 alpha 时为 255。推荐在 shell 中加引号。JSON 色值为四个 0–255 整数，进入核心时转换到线性光。未知参数/枚举拒绝，JSON 的 filter、expand、anchor、background 必须显式提供，不随将来的 CLI 默认值变化。`steps[].params` 返回完整规格（resize 的未指定维度为 null）。这些规格保持逻辑步骤边界，供后续 ops 记录使用。

```sh
pic-cli crop --input photo.png --x 20 --y 10 --width 640 --height 480 --output cropped.png --json
pic-cli resize --input photo.png --width 1600 --filter bilinear --output resized.png --png-compression 6 --json
pic-cli rotate --input photo.png --degrees -30 --background '#00000000' --output rotated.png --json
pic-cli flip --input photo.png --axis vertical --output flipped.png --json
pic-cli canvas --input photo.png --width 1920 --height 1080 --anchor bottom_right --background '#ffffff' --output canvas.png --json
```

管线 JSON 示例（crop 的区域需在输入画布内）：

```json
{
  "schema_version": 1,
  "operations": [
    {"op":"crop","op_version":1,"target":"canvas","params":{"x":20,"y":10,"width":640,"height":480}},
    {"op":"resize","op_version":1,"target":"canvas","params":{"width":320,"height":null,"filter":"bilinear"}},
    {"op":"rotate","op_version":1,"target":"canvas","params":{"degrees":90,"expand":true,"filter":"nearest","background":[0,0,0,0]}},
    {"op":"flip","op_version":1,"target":"canvas","params":{"axis":"horizontal"}},
    {"op":"canvas","op_version":1,"target":"canvas","params":{"width":400,"height":400,"anchor":"center","background":[255,255,255,255]}}
  ]
}
```

```sh
pic-cli run --input photo.png --pipeline geometry.json --output result.jpg \
  --jpeg-quality 90 --jpeg-background '#ffffff' --json
```

编码参数同样适用于 `run`；管线 JSON 只描述操作，不接受未声明的顶层 encoding 字段。操作不重排、不融合；一次运行只在导入/最终导出处执行 codec。逐条命令会在每步导出时量化，因此涉及插值时，连续多个命令未必等于只导出一次的多步管线。单命令与等价 JSON 单步严格等价；整数复制操作构成的多步链与逐条 PNG 命令也严格等价。

## 采样与旋转

工作表示仍为 `linear_srgb_rgba32f_straight_v1`。几何操作不经过 RGBA8 中间图像；采样时使用临时预乘 alpha，结束转回 straight alpha。负 RGB/HDR 保留，最终输出才裁切并量化。alpha 仅在过滤后修正浮点舍入至 `[0,1]`；无法表示的非有限工作样本返回 `invalid_argument`。浮点插值不承诺跨 CPU/编译器逐位相同。

- 裁剪、翻转、nearest 缩放、扩展画布的直角旋转和 canvas 复制保留来源像素的全部 f32 位模式，包括负零、alpha=0 的隐藏 RGB。
- 相同尺寸 resize、0°/±360° rotate 直接共享原像素；180° 旋转始终精确。90°/270° 在 expand=true 或正方形画布上精确复制。
- bilinear 过滤混合的是 `(r*a,g*a,b*a,a)`，不会让透明黑或隐藏颜色渗入边缘；过滤结果 alpha=0 时 RGB 统一为 0。它不保留全透明区域的隐藏色，这是明确的重采样语义；identity/整数复制不受影响。

缩放的 nearest 在每轴取 `floor((d+0.5)*source_size/destination_size)`，在精确中点选择右/下像素。bilinear 使用锁定的 **fast_image_resize 5.5.0**，以 F32x4 的线性预乘像素执行 triangular convolution。缩小时按比例扩大核以抗锯齿，边界截断核并重新归一化；放大时使用相邻两点。禁用库内重复 alpha 转换，不开启 Rayon；用安全的 bytemuck 切片视图连接既有 Raster 和采样器，不建立第二个像素状态。选择 5.5.0 是因为其声明的 Rust 最低版本为 1.87，兼容 workspace 的 1.88 要求；本机实际验证使用已安装的编译器。此任务实现并验证正确性，任务 11 的后续性能测量见 [当前报告](performance.md)，不宣称比其他引擎更快。上游要求调用方处理线性色彩转换，参见 [fast_image_resize 文档](https://docs.rs/fast_image_resize/5.5.0/fast_image_resize/)。

任意角旋转默认输出：

```text
W' = ceil(abs(W*cos θ) + abs(H*sin θ))
H' = ceil(abs(W*sin θ) + abs(H*cos θ))
dx = x + 0.5 - W'/2; dy = y + 0.5 - H'/2
source_x =  cos θ*dx + sin θ*dy + W/2
source_y = -sin θ*dx + cos θ*dy + H/2
```

使用 f64 计算角度与坐标。输出中心与原中心对齐，ceil 带来的空隙均分到两侧。`--keep-size` 使用 `W'=W,H'=H` 并裁掉越界内容。非正方形画布固定大小转 90°/270° 时，也遵循上述中心采样；奇偶尺寸不同可落在半像素位置，不保证精确复制。

nearest 用 `floor(source_x),floor(source_y)`；bilinear 以像素中心为格点做四点插值。每个越界采样点替换为 background，不夹取到边缘。背景不提前铺在图像的透明区域下面；它只定义画布外采样。旋转不额外实现高阶滤镜或面积覆盖率抗锯齿。

canvas 的九个锚点为 `top_left, top, top_right, left, center, right, bottom_left, bottom, bottom_right`。每轴 source→destination 偏移分别是 start=`0`、center=`floor((new-old)/2)`、end=`new-old`。例如 3→4 的居中偏移为 0（多出来的像素在右/下）；3→2 的偏移为 -1（左/上裁掉一像素）。background 只填未被源画布覆盖的位置，**不会改变已复制像素的透明度**；需要不透明 JPEG 时用 jpeg-background。

## 真实文件与编码

| 项目 | 接受、转换或拒绝 |
| --- | --- |
| PNG | 静态 8-bit gray/gray-alpha/RGB/RGBA，支持交错；调色板、1/2/4/16-bit、APNG 拒绝 |
| JPEG | 8-bit grayscale/RGB/YCbCr，baseline/extended-sequential/progressive；CMYK/YCCK、高位深及其他编码模式拒绝 |
| EXIF | JPEG APP1 EXIF 和 PNG eXIf：支持 classic TIFF 大小端，检查 IFD0 的 orientation 与 Exif 子 IFD 色彩；主方向 1–8 归一化一次，缺省方向 1；多个 EXIF 块、重复/越界/无效 orientation、坏目录/类型/数据偏移返回 unsupported_metadata |
| EXIF 范围 | 不递归读取缩略图、GPS、MakerNote 等非主图目录，不承诺全部 EXIF 校验或保留；导出删除源元数据并返回 metadata_not_preserved 警告 |
| 方向观察 | `info.width/height` 为归一化尺寸，`stored_width/stored_height` 为存储尺寸，`exif_orientation` 为原方向（有 EXIF 无方向时为 1，无 EXIF 为 null）；转换方向 2–8 返回 orientation_applied 警告；操作坐标只基于归一化像素 |
| 色彩 | PNG sRGB 标记或 EXIF ColorSpace=1 可声明 sRGB；没有声明则假设 sRGB 并警告 assumed_srgb；不做 ICC 转换 |
| 未支持色彩 | PNG/JPEG ICC（包括 sRGB ICC）、PNG gAMA/cHRM/cICP/mDCV/cLLI、EXIF 非 sRGB/未校准 ColorSpace、EXIF ICC/transfer-function/white-point/primaries/gamma 明确返回 unsupported_color，即使其他标签声称 sRGB |
| PNG 输出 | RGBA8；`--png-compression 0..9`，默认 6，0 无压缩，1–9 为 deflate level；自适应行滤波。压缩级别不改变解码像素，不承诺文件大小单调或编码字节稳定 |
| JPEG 输出 | RGB8；`--jpeg-quality 1..100`，默认 90；有损重编码，不承诺通用像素误差上限 |
| 透明 JPEG | 只要工作 alpha < 1，未提供背景就返回 alpha_not_supported；`--jpeg-background '#RRGGBB'` 必须不透明，按 `C = source*C_alpha + background*(1-C_alpha)` 在线性光铺底后转回 sRGB；不修改工作 Raster，发生铺底时返回 alpha_flattened |
| 编码参数验证 | PNG 拒绝 jpeg-quality/jpeg-background；JPEG 拒绝 png-compression；越界质量、压缩或不透明背景校验失败不发布输出 |

没有将 ICC、高位深或 unsupported format 静默降级。源文件和导出文件按内容解码验收，不以扩展名或压缩字节相同作为像素保真依据。

## 资源与失败语义

沿用 [基础限制和原子发布](foundation-contract.md)：单边 32,768，40,000,000 像素，基础工作估算 `32*像素数 <= 1 GiB`，默认最多 10,000 步。每个中间画布都受限，不允许先造出超限画布再通过后续 crop 缩小来绕过限制。

普通几何另按同时存活的 source+destination 的 16 字节像素缓冲检查。bilinear 缩放在分配前保守计入原图、预乘副本、目标、两轴 pass 的最大交叉缓冲 `max(source_width*target_height, target_width*source_height)` 和系数余量 `64*(source_width+source_height+target_width+target_height)+16` 字节。即使输入和输出各自满足限制，极端宽高比转换也可能返回 resource_limit。Pipeline 保存构造限额，文件执行入口的更小限额同样生效。乘加及推导尺寸检查溢出；自有工作像素使用可失败的预分配。

这是准入预算，不是严格进程 RSS 或操作系统 OOM 保证；codec、库内 scratch 分配、已被调用方持有的共享 Raster 和其他运行时开销仍受宿主约束。所有步骤成功后才编码、创建同目录临时文件并原子发布。几何校验、资源限制、颜色/方向拒绝或编码失败不会替换已有目标；错误继续使用稳定 JSON code，执行中的错误消息包含 `operations[index]`。

## 验收证据

所有测试在临时目录生成素材、JSON 和输出。测试源码为 [核心几何](../crates/pic-core/tests/geometry.rs)、[核心与 codec](../crates/pic-core/tests/core.rs)、[CLI 独立进程](../crates/pic-cli/tests/cli.rs)。

| 验收 | 证据 |
| --- | --- |
| 全部几何的 CLI/JSON、参数与能力描述 | `every_geometry_command_matches_a_json_step_in_separate_processes` 覆盖 crop、nearest/bilinear resize、直角/任意角/固定大小 rotate、双轴 flip、扩展/裁切 canvas；比较归一化步骤参数与解码 RGBA；help/capabilities 回归仍运行 |
| 坐标、顺序、背景和浮点精度 | `orthogonal_geometry_copies_coordinates_and_every_float_bit` 使用 3×2 解析样本（HDR、负值、负零、隐藏色）；canvas 九锚点和奇数裁切；`multistep_pipeline_matches_sequential_cli_processes_and_known_coordinates` 对六步 crop→resize→rotate→flip→flip→canvas 的逐条新进程结果和单次 JSON 结果进行逐像素坐标断言 |
| 透明边缘与线性采样 | `bilinear_resize_is_premultiplied_linear_light_with_no_black_or_hidden_color_fringe` 检查红色/透明蓝边缘、一维/二维缩放、黑白平均在线性光为 0.5、导出为 sRGB 188；任意角旋转另有解析 alpha、中心与背景断言 |
| EXIF 与真实 codec | `real_jpeg_exif_all_eight_orientations_are_normalized_before_info_and_crop` 真实编码 3×2 JPEG，八方向 × 大小端，覆盖扫描后的 APP1，检查尺寸、每像素置换和操作前方向；另测 PNG eXIf、重复/坏 EXIF、ICC/色彩/格式/位深拒绝 |
| PNG/JPEG 往返和输出参数 | 原有 256 级通道/隐藏 RGB PNG 精确往返和 JPEG↔PNG/灰度回归；新增 PNG 0/1/6/9 各级实际编码与解码、压缩生效、JPEG 线性背景铺底、默认拒绝 alpha、格式不匹配参数测试 |
| 失败不覆盖与资源限制 | `geometry_errors_are_structured_and_never_replace_outputs` 在独立进程验证越界、溢出、非法尺寸、格式参数、资源限额及已有文件字节保持；核心测试中间尺寸及交叉 scratch 预算；原有部分写入失败、并发 no-clobber 与符号链接测试继续运行 |

2026-09-19，当前独立任务 worktree，Linux x86_64 / rustc 1.98.1，实际完成：

| 验证命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，无 warning |
| `cargo test --workspace` | 40 个测试通过：21 CLI + 18 核心集成 + 1 发布失败单元测试 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过；归档和暂存后另查 `git diff --cached --check` |
| `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli` | 实际 release 二进制的 21 个独立进程集成测试全部通过 |

无外部 blocker。当前只验证本机 Linux；ICC 转换、高位深/其他格式、高阶插值和跨平台浮点逐位一致性不在实现范围。极端宽高比可能触发保守 scratch 预算。工程持久化及 expected_revision 的既定契约未改变，它们已在后续 [工程任务](project-history.md) 实现。
