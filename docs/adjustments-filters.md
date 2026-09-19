# 调色与滤镜契约（任务 03）

这些操作扩展既有 `OperationSpec → Operation → Pipeline → Raster`，全部为 `op_version=1`、`target="canvas"`。单操作命令和 JSON 使用同一参数类型、验证与执行核心。曝光、亮度、对比度和饱和度由一个 `adjust` 操作提供；其余操作为 `levels`、`curves`、`grayscale`、`invert`、`blur`、`sharpen`。

## 参数和默认值

所有数字必须有限，范围包含端点，另列的关系约束也必须满足。CLI 使用下表默认值；JSON **必须显式提供全部字段**，包括 `channel`，不会从未来的 CLI 默认值推导重放行为。结果 `steps[].params` 返回完整规范化规格。灰度与反相参数只能是空对象 `{}`。未知字段、通道、版本、目标、操作或不支持的滤镜/边界模式均拒绝。

| 操作 | CLI 选项 / JSON 字段 | 范围 | CLI 默认值 |
| --- | --- | --- | --- |
| adjust | `--exposure` / `exposure` | EV `[-20,20]` | 0 |
| adjust | `--brightness` / `brightness` | 线性 RGB 加法偏移 `[-1,1]` | 0 |
| adjust | `--contrast` / `contrast` | 绕线性值 0.5 的倍率 `[0,10]` | 1 |
| adjust | `--saturation` / `saturation` | 相对 Rec.709 亮度的倍率 `[0,10]` | 1 |
| levels | `--channel` / `channel` | `rgb, red, green, blue` | rgb |
| levels | `--input-black` / `input_black` | `[0,1]`，严格小于 input_white | 0 |
| levels | `--input-white` / `input_white` | `[0,1]`，严格大于 input_black | 1 |
| levels | `--gamma` / `gamma` | `[0.1,10]` | 1 |
| levels | `--output-black` / `output_black` | `[0,1]`，不大于 output_white | 0 |
| levels | `--output-white` / `output_white` | `[0,1]`，不小于 output_black | 1 |
| curves | `--channel` / `channel` | `rgb, red, green, blue` | rgb |
| curves | `--points` / `points` | 2–256 个 `[x,y]`；x/y 都在 `[0,1]`；x 从 0 到 1 严格递增 | `[[0,0],[1,1]]` |
| grayscale / invert | 无 | JSON `{}` | 无 |
| blur | `--sigma` / `sigma` | 高斯标准差 `[0,100]` 像素 | 1 |
| sharpen | `--sigma` / `sigma` | 同 blur | 1 |
| sharpen | `--amount` / `amount` | 锐化强度 `[0,10]` | 1 |

`rgb` 表示独立处理三个 RGB 通道；`red/green/blue` 只处理一个通道。alpha 不可作为调色通道；灰度、反相和 adjust 固定作用于全部 RGB。曲线 CLI 的 points 是加引号的 JSON 数组，而管线中直接写数组。

```sh
pic-cli adjust --input photo.png --exposure 0.5 --saturation 1.1 --output adjusted.png --json
pic-cli adjust --input photo.png --brightness -0.05 --contrast 1.2 --output contrast.png --json
pic-cli levels --input photo.png --input-black 0.02 --input-white 0.9 --gamma 1.2 --output levels.png --json
pic-cli curves --input photo.png --channel red --points '[[0,0],[0.25,0.4],[1,1]]' --output curves.png --json
pic-cli grayscale --input photo.png --output gray.png --json
pic-cli invert --input photo.png --output inverted.png --json
pic-cli blur --input photo.png --sigma 2 --output blurred.png --json
pic-cli sharpen --input photo.png --sigma 1 --amount 0.75 --output sharp.png --json
```

## 颜色、精度和 alpha

工作状态仍是 `linear_srgb_rgba32f_straight_v1`：线性光 sRGB、RGBA32F、straight alpha。调色参数和曲线坐标均在线性域，不能直接把非线性 sRGB 字节、Photoshop 滑杆百分数或其他软件 gamma 规则代入。每个逻辑操作以标量 f64 计算，再写回 f32；高斯两轴间保留预乘 f64 样本。操作之间不转成 RGBA8、不裁切 RGB 到 `[0,1]`，最终 PNG/JPEG 导出才执行原有裁切和量化。非有限 f32 结果报 `invalid_argument`，不会保存无效 Raster。

颜色操作保持 alpha 的每一位，包括负零，并会变换 alpha=0 的隐藏 RGB。色阶/曲线保持未选通道的每一位。模糊会过滤 alpha，锐化保持输入 alpha，详见下文。所有操作保持画布尺寸。

以下恒等设置直接共享原 Raster，保留全部 f32 位模式，包括 HDR、负值、负零、次正规数和透明隐藏 RGB：

- adjust 的四个默认值；单个中性控制在计算中也会跳过。
- levels 的 gamma=1 且输入黑/白点分别等于输出黑/白点。
- curves 的所有点满足 x=y。
- blur 的 sigma=0；sharpen 的 sigma=0 或 amount=0。

### 曝光、亮度、对比度和饱和度

一个 adjust 是一个逻辑步骤，内部固定顺序为：

```text
C1 = C * 2^exposure
C2 = C1 + brightness
C3 = (C2 - 0.5) * contrast + 0.5
Y  = 0.2126*R3 + 0.7152*G3 + 0.0722*B3
C4 = Y + saturation * (C3 - Y)
```

C 分别代表 R/G/B。实际 Y 按 `G + 0.2126*(R-G) + 0.0722*(B-G)` 求值，以保持中性灰；grayscale 操作使用同一权重与求值顺序。saturation=0 得到灰度，contrast=0 得到线性 0.5。contrast 的中心不是非线性 sRGB 128。需要其他顺序时，使用多条 adjust 并为其余控制提供中性值；每条仍保留独立逻辑边界和 f32 结果。

### 色阶和曲线

色阶在选中通道按下式求值：

```text
t  = (C - input_black) / (input_white - input_black)
g  = sign(t) * abs(t)^(1/gamma)
C' = output_black + (output_white - output_black) * g
```

gamma=1 时直接使用 t。输入黑点映射输出黑点，输入白点映射输出白点；输出两端相等时返回该常数。为遵守工作像素精度契约，**不把 t 限制在 `[0,1]`**：负 t 使用有符号幂，超过白点仍延伸。默认色阶因此也保留范围外样本。这是本产品的明确语义，不承诺其他编辑器的裁切式色阶或滑杆兼容。

曲线在每对相邻控制点之间线性插值：`y0 + (C-x0)*(y1-y0)/(x1-x0)`；实现先计算斜率再乘输入差。恰落在控制点时直接返回该点 y。y 允许非单调；不自动排序或合并重复 x，不使用三次样条。C<0 或 C>1 时延伸首段或末段，常数段继续为常数，不裁切输入/输出。全部 x=y 的曲线为精确恒等。

### 灰度和反相

grayscale 把三个 RGB 都设为上述 Rec.709 Y。invert 对每个线性 RGB 计算 `1-C`，不是 `255-byte` 的非线性反色。因此线性 0.25 变成 0.75，而非将对应 sRGB 字节逐字节取反；负值和 HDR 仍保留范围外结果。

## 高斯模糊与锐化

sigma>0 时，核半径 `r=ceil(3*sigma)`，共有 `2r+1` 个 taps。整数偏移 d 的未归一化权重是 `exp(-0.5*(d/sigma)^2)`，用 f64 按 `d=-r..r` 求和后归一化。先水平、再垂直遍历，**每一个越界 tap 都夹取到最近边缘像素**；不是补透明、镜像、环绕或截断核后重归一化。sigma=0 是精确恒等。

水平 pass 从 straight RGBA32F 转成 `(R*a,G*a,B*a,a)` 并累积到 RGBA64F 临时缓冲；垂直 pass 对此缓冲做同一核过滤。最后以未舍入 alpha 反预乘 RGB，alpha 的浮点舍入修正到 `[0,1]`，再保存 f32。如果保存后的 alpha=0，RGB 统一为 0（也包括小到舍入为零的覆盖率）。透明隐藏色不会进入可见边缘；例如不透明红与透明蓝的模糊边缘保持红色，不出现蓝/黑边。负值和 HDR 色彩继续保留。

sharpen 是无阈值的 unsharp mask：B 为同一 blur v1 的 f32 输出，对输入 alpha>0 的 RGB 计算 `C'=C+amount*(C-B)`；保留高于 1/低于 0 的过冲。原 alpha 位模式保持，alpha=0 的隐藏 RGB 也原样保持。sigma=0 或 amount=0 直接返回原 Raster。

资源准入在分配前检查：点操作同时存活原图和输出，按 `32*N` 字节；blur 按原图 `16*N`、水平缓冲 `32*N`、输出 `16*N` 和核 `8*(2r+1)` 字节计入预算。sharpen 在 blur 后需要 `48*N` 字节（原图、模糊图、锐化输出），峰值不超过上述 blur 预算。所有自有缓冲使用可失败预分配，pipeline 的构造限额和调用方更小限额都生效。全进程 RSS 的既有边界见 [基础执行契约](foundation-contract.md)。

当前实现按标量顺序执行，不融合逻辑操作、不开启线程池。两轴高斯是上述核的明确实现，计算量随像素数和 sigma 增加；任务 11 已测量 1080p sigma=12 模糊及 1080p/4K 调色，见 [当前性能报告](performance.md)。浮点结果不承诺跨 CPU/编译器逐位相同。

## 有序管线和发布

示例（输入至少 640×480）：

```json
{
  "schema_version": 1,
  "operations": [
    {"op":"crop","op_version":1,"target":"canvas","params":{"x":0,"y":0,"width":640,"height":480}},
    {"op":"resize","op_version":1,"target":"canvas","params":{"width":320,"height":null,"filter":"bilinear"}},
    {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":0.5,"brightness":0,"contrast":1,"saturation":1.1}},
    {"op":"levels","op_version":1,"target":"canvas","params":{"channel":"rgb","input_black":0,"input_white":1,"gamma":1.2,"output_black":0,"output_white":1}},
    {"op":"curves","op_version":1,"target":"canvas","params":{"channel":"blue","points":[[0,0],[0.5,0.55],[1,1]]}},
    {"op":"blur","op_version":1,"target":"canvas","params":{"sigma":0.5}},
    {"op":"sharpen","op_version":1,"target":"canvas","params":{"sigma":1,"amount":0.5}}
  ]
}
```

```sh
pic-cli run --input photo.png --pipeline editing.json --output result.png --json
```

一次 run 只导入/解码一次，执行全部逻辑步骤后只编码并发布一次；没有中间文件、隐藏重编码或额外图像状态。单命令和等价 JSON 单步的参数与像素相同；连续多个单命令会在各次导出量化，因此不保证与多步管线相同。例如 +20 EV 再 -20 EV 的一次性管线可精确恢复原工作样本；拆成两个 PNG 命令时，高光已在第一次导出裁切。

参数验证在图像 I/O 前完成；运行时无法表示的非有限结果、超限缓冲等在执行步骤上返回 `operations[index]` 错误。所有步骤成功才进入编码和原子发布；失败 `ok=false`、`data=null`、非零退出码，不创建成功目标，已有文件保持原样。JSON 语法中的 NaN/Infinity/溢出数值返回 `invalid_json`，类型/范围错误返回 `invalid_argument`。不支持 alpha 的 JPEG 导出仍需显式背景。

源素材/不可变 ops、expected_revision 和检查点精度的既有契约不变。任务 03 当时未实现工程持久化；当前 [工程历史](project-history.md) 与 [重放/检查点](replay-preview.md) 已复用这些版本化操作和逻辑边界。

## 验收证据

核心像素测试：[adjustments.rs](../crates/pic-core/tests/adjustments.rs)。真实独立进程测试：[cli.rs](../crates/pic-cli/tests/cli.rs)。编解码计数测试：[codec.rs](../crates/pic-core/src/codec.rs)（计数器仅在测试编译启用）。

| 验收 | 测试证据 |
| --- | --- |
| 全部操作、CLI/JSON、帮助与能力 | `every_photo_command_matches_its_explicit_json_parameters_and_pixels`：18 组默认/非默认调用，覆盖全部七种操作、四个 adjust 控制、各选色通道，对照完整参数和解码像素；检查 help 和参数能力声明 |
| 恒等与内部精度 | `neutral_parameters_preserve_every_float_bit_and_shared_storage`；`ordered_steps_retain_hdr_precision_and_each_logical_boundary`；真实 CLI 的 +20/-20 EV PNG 精确往返 |
| 小色块、色阶与曲线 | `adjustment_controls_have_independent_linear_rgb_expectations`；灰度/反相原色测试；色阶黑白点、gamma 和范围外解析预期；非单调曲线控制点、段间插值和端点外延；未选通道位级保持 |
| 透明像素、模糊边缘与锐化 | 独立列出的 sigma=1 高斯系数；一维冲激、边缘阶跃及二维样本；红/透明蓝、半透明加权和全透明隐藏色；unsharp 的负值/高亮及 alpha 位级保持 |
| 顺序和 codec 次数 | `photo_pipelines_keep_step_order_and_never_quantize_or_clip_between_steps`；`multistep_file_run_decodes_and_encodes_once` 实际计数一次 decode、一次 encode，并保留五个逻辑步骤 |
| 几何+调色+滤镜真实导出 | `geometry_adjustment_and_filters_export_known_pixels_to_png_and_jpeg`：crop→flip→rotate→adjust→blur→sharpen，PNG 的独立预期灰值 171/85、alpha 128；JPEG 显式铺白后预期 218/195，编码容差 2 |
| 非法参数与失败无输出 | 核心覆盖必填/未知字段、范围、通道、曲线形状和所有浮点字段的 NaN/±Inf；真实 CLI 验证无新输出/旧输出不变、非法 JSON、运行时溢出步骤定位及编码/发布未执行；额外滤镜缓冲限额测试 |

完整校验结果在当前 todo 归档中记录。已知范围限制：当前仅本机 Linux 验证；不支持三次曲线、其他滤镜核/边界模式、ICC/高位深输入；固定素材的照片尺寸基线已建立，见 [性能报告](performance.md)；无 Photoshop 像素兼容承诺。
