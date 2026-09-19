# 图层、合成、选区与蒙版

任务 06 扩展同一 `OperationSpec → Pipeline → Document` 核心。Document 是从原始资产和不可变 ops 求值得到的完整编辑状态；渲染 Raster 是输出，不是另一份编辑状态。没有模型、自动选区、智能抠图或图像生成调用。分组、文字、剪贴蒙版和调整层留给任务 07。

## 命令与稳定目标

```sh
pic-cli project create --input background.png --output work.pic --json
pic-cli project layer add work.pic --id subject --source subject.png --name Subject --expect-revision r0 --json
pic-cli project layer transform work.pic --target subject --x 100 --y 80 --scale-x 0.5 --scale-y 0.5 --degrees 15 --expect-revision r1 --json
pic-cli project mask set work.pic --target subject --source mask.png --expect-revision r2 --json
pic-cli project layer set work.pic --target subject --opacity 0.8 --blend multiply --expect-revision r3 --json
pic-cli project selection set work.pic --space subject --region 10,20,100,80 --expect-revision r4 --json
pic-cli project layer edit work.pic --target subject --op invert --expect-revision r5 --json
pic-cli project preview work.pic --target subject --output layer.png --json
pic-cli project preview work.pic --target mask:subject --output coverage-preview.png --json
pic-cli project checkpoint work.pic --json
pic-cli project export work.pic --output final.png --json
```

示例 revision 假设每次命令只提交一步。实际使用返回的 revision，所有修改命令仍须 `--expect-revision`（别名 `--expected-revision`）；`--revision` 可选续编基点，但 expected revision 始终比较当前指针。CLI 子命令仅创建一个 OperationSpec 并调用现有 Project::apply；JSON apply 可把多步组成一个撤销组。查询、预览、失败请求不进入历史。

初始图像的图层 ID 为 `base`。新增 ID 由调用方显式指定：1–128 个 ASCII 字母、数字、`_` 或 `-`，不能为 `canvas`，沿同一历史祖先路径不得复用已使用／删除的 ID。不同 revision 中的同一 ID 表示对应图层；引用必须连同所观察的 revision 使用。显示名称可重复，也可随时改变，不参与任何定位。没有隐式“当前选中层”：每次编辑都显式写 target。

Document 的 `layers` 顺序为从底到顶。`layer reorder --target ID --before OTHER` 把 ID 放在 OTHER 紧下方；省略 `--before` 移到最顶。自身和不存在的参照 ID 返回 `invalid_target`。`layer remove` 删除图层及附属蒙版；若当前选区以该层为坐标空间，同时清除选区。撤销恢复完整状态。允许移除全部图层，画布输出透明。

`project inspect` 增加 `document`：画布宽高、layered 状态、有序图层 ID／名称／尺寸／显隐／opacity／blend／transform／mask ID／双向坐标映射、选区和已使用 ID 集合。这些字段来自恢复的 Document，不写入 manifest 作为平行权威状态。

## 操作参数

所有操作 `op_version=1`，管线 `schema_version=1`。参数对象拒绝未知字段。资源路径相对管线文件所在目录；单命令相对 cwd。规范化记录会补齐 nullable 参数为 null；transform 的全部字段在 JSON 中必填。

| op | target | params |
| --- | --- | --- |
| layer_add | canvas | `{"id":"subject","source":"subject.png","name":"Subject"}`；在最顶新增，默认可见／opacity=1／normal／恒等变换 |
| layer_set | 图层 ID | `name`、`visible`、`opacity`、`blend` 均可省略或 null，只修改非 null 字段；opacity 有限且在 [0,1] |
| layer_transform | 图层 ID | `x,y,scale_x,scale_y,degrees,flip_x,flip_y,filter`；替换完整变换，不累乘旧值 |
| layer_reorder | 图层 ID | `{"before":"other"}` 或 `{"before":null}` |
| layer_remove | 图层 ID | `{}` |
| mask_set | 图层 ID | `{"source":"mask.png"}`；完整替换附属蒙版 |
| mask_remove | 图层 ID | `{}`；无蒙版时幂等 |
| selection_set | canvas | `{"space":"canvas","regions":[{"x":10,"y":20,"width":100,"height":80}]}`；space 也可为图层 ID |
| selection_clear | canvas | `{}` |
| composite | 旧单画布 canvas 或图层 ID | `source,mask,opacity,blend,transform`；mask 可省略或 null |
| 原有像素操作 | 旧单画布 canvas 或图层 ID | 原有参数不变；图层 ID 表示本地像素，画布尺寸不随本地图层像素编辑改变 |

完整可执行 JSON 示例：

```json
{
  "schema_version": 1,
  "operations": [
    {"op":"layer_add","op_version":1,"target":"canvas","params":{"id":"subject","source":"subject.png","name":"Subject"}},
    {"op":"mask_set","op_version":1,"target":"subject","params":{"source":"mask.png"}},
    {"op":"layer_transform","op_version":1,"target":"subject","params":{"x":3,"y":1,"scale_x":1,"scale_y":1,"degrees":90,"flip_x":false,"flip_y":false,"filter":"nearest"}},
    {"op":"selection_set","op_version":1,"target":"canvas","params":{"space":"subject","regions":[{"x":0,"y":0,"width":1,"height":1}]}},
    {"op":"invert","op_version":1,"target":"subject","params":{}}
  ]
}
```

CLI 对应入口：`project layer add/set/transform/reorder/remove/edit`、`project mask set/remove`、`project selection set/clear`。`layer edit --op ... --params '{...}'` 和 JSON target 共用校验及执行器。CLI `layer transform` 的默认值是 x/y/degrees=0、scale_x/scale_y=1、flip_x/flip_y=false、filter=bilinear；每次调用替换全部变换。`selection set --region x,y,w,h` 可重复，用于矩形并集。

## 像素、混合与蒙版

图层存储未预乘的线性 sRGB RGBA32F；负值、HDR 和隐藏 RGB 留在独立图层像素中。显隐、opacity、变换和蒙版均不修改该像素素材。不可见图层对合成没有贡献。

每个来源像素先以蒙版 coverage 乘 alpha，再在预乘线性颜色与 alpha 上采样，最后还原 straight RGB。不得分别插值颜色和蒙版后再相乘，否则被遮住的颜色会污染边缘。采样范围外为透明黑。opacity 在采样后乘有效 alpha；零 alpha 来源不影响底图。

对底色 `b`、来源 `s`，底 alpha `ab`、采样后来源 alpha 乘 opacity 得 `as`：

```text
ao = as + ab * (1-as)
Co = (as*(1-ab)*s + as*ab*B(b,s) + (1-as)*ab*b) / ao
normal:   B = s
multiply: B = b*s
screen:   B = b+s-b*s
overlay:  B = 2*b*s                     (b <= 0.5)
          B = 1-2*(1-b)*(1-s)          (b > 0.5)
```

逐 RGB 通道在线性光计算，中途不裁切。完全透明底图直接采用来源颜色，不因 multiply 等模式产生黑底；零输出 alpha 采用透明黑。非有限 f32 结果拒绝。这里不承诺与 Photoshop 的混合色彩设置逐像素一致。

外部蒙版使用受支持的 8 位灰度 PNG/JPEG；RGB/RGBA 文件的所有像素须 R=G=B。先按已有规则归一化 EXIF，尺寸必须与目标图层本地像素严格一致，不自动缩放。灰度是**编码值 coverage**：`coverage=(gray/255)*(alpha/255)`，8 位 128 是约 0.502，不做 sRGB 解码成为约 0.216。彩色蒙版或尺寸不匹配返回 `invalid_mask`；格式、色彩、方向和资源准入错误沿用现有 codec 错误。JPEG coverage 带有 JPEG 有损解码误差，精确覆盖率建议使用 PNG。

每层至多一个外部蒙版，稳定预览 ID 为 `mask:<layer ID>`。蒙版与图层共享本地尺寸及变换。图层原始像素不受 mask_set/remove 破坏，撤销／重做可恢复完整蒙版。

单次合成使用同一层采样／混合代码：

```sh
pic-cli composite --input background.png --overlay subject.png --mask mask.png \
  --x 24 --y 16 --opacity 0.8 --blend screen --output composite.png --json
```

`composite` 是对目标本地像素的直接合成操作，输出尺寸不变；要持续独立编辑来源，请用 layer_add。工程中禁止 composite 隐式压平已有多层画布；可显式指向其中一层。存在选区时拒绝 composite，须先清除选区。

## 变换、选区与坐标

采用连续像素边界坐标，左上原点、x 向右、y 向下，像素 `(i,j)` 的中心为 `(i+0.5,j+0.5)`。无损变换依次执行：在本地图像边界内翻转 → x/y 缩放 → 围绕本地原点顺时针旋转 → 平移 x/y。原点旋转可使内容落在负坐标；画布之外仅在渲染时裁切，不删除图层内容。

x/y 有限，绝对值不超过 1e9；scale_x/scale_y 在 [1e-6,1e6]，翻转由独立布尔量表达；degrees 在 [-360,360]。正交角使用精确 sin/cos，避免坐标漂移。`nearest` 取反向映射点所在像素；`bilinear` 使用四点预乘采样，图层缩小时暂不加宽低通核。原有破坏性 `resize` 仍使用其抗混叠内核，二者采样范围明确区分。

矩阵序列化为 `[a,b,c,d,e,f]`：`x'=a*x+c*y+e, y'=b*x+d*y+f`。返回正逆矩阵，支持缩放／旋转／翻转，不能仅用平移或宽高比替代真实映射。

选区是最多 1024 个有限、正宽高的半开矩形并集。设置时必须完整位于命名空间内；空数组表示没有像素入选，selection_clear 表示所有像素入选。像素操作先计算完整图层结果，再按目标本地像素中心映射到选区空间决定写入哪些像素。滤镜读取完整图层，选区只限制写入。选区本身不裁切渲染，也不限制图层属性／变换／蒙版设置。图层空间选区跟随该层变换；画布空间选区固定在画布上，可用于编辑其他变换后的图层。

附有蒙版或存在选区时，图层的破坏性 crop/resize/rotate/flip/canvas 返回 `invalid_argument`，避免隐式改变绑定坐标或尺寸。无损 layer_transform 可直接使用；需要破坏性几何时，先 mask_remove／selection_clear，再编辑及重新提供蒙版。此限制是明确的首版范围。

## 旧单画布工程及多层画布语义

已有 schema_version=1 的单 canvas 工程无需迁移，旧 manifest 和 ops 不改写。初始源图求值为 `base` 层。在发生图层／蒙版／选区操作或显式图层目标编辑之前，canvas 操作完全保留原 P0 语义与每个 f32 位，包括 identity、隐藏颜色、几何尺寸和历史边界；复用原单 Raster 快照格式。

进入 layered 状态后不因删除至一层而隐式回退。canvas 目标仅接受：

| 操作 | 多层语义 |
| --- | --- |
| identity | 保留完整编辑状态 |
| crop | 矩形须在当前画布内；改变画布尺寸，把所有层及画布空间选区平移 `(-x,-y)`，保留各层本地像素与蒙版；选区平移后允许部分在画布外 |
| canvas | 保留九锚点规则，改变尺寸并平移各层及画布空间选区；背景参数必须是透明黑 `[0,0,0,0]`，需要颜色时使用独立背景层 |
| 其他像素操作 | `invalid_target`；明确指定图层 ID 后执行本地像素操作，或使用 layer_transform |

上述操作绝不把独立图层静默烘焙成一张图片。`project revise` 仍重放新的后缀并生成新 ops/revision，旧版本保持可读；新参数导致依赖尺寸或目标不合法时整组失败。

## 预览、持久化与缓存

preview target 支持 `canvas`、图层 ID 和 `mask:<layer ID>`，输出始终使用该 revision 的**画布坐标**，region 也始终为画布区域。图层预览隔离该层，保留变换、显隐、opacity 与蒙版；混合采用相同 source-over 规则，因为没有其他底层，效果是其独立贡献。蒙版预览显示独立 coverage 的不透明灰度，不乘图层 opacity、可见性或图层本身 alpha，画布外／层外 coverage 为黑；8 位 128 预览回 128。缩小蒙版预览时先对 coverage 采样，再转换为显示灰度；0／255 的平均值是 128，不会因提前进行显示 gamma 转换而变成约 188。

preview 和 export 都调用 Document::render。区域裁剪与缩放继续用原有 Pipeline 的 crop/resize，临时观察步骤不进入历史。JSON 保留原 preview_to_canvas / canvas_to_preview 的 scale、offset，同时增加 layer_to_canvas、canvas_to_layer、preview_to_layer、layer_to_preview 六系数矩阵；canvas 目标的四个新增字段为 null。层／蒙版的局部分析点可按这些矩阵映射。缓存命中仍返回完全相同的坐标信息。

每次导入图层／蒙版／composite 素材先读取明确字节，按 SHA-256 去重保存到 `assets/`，ops 的 source/mask 规范化为 `asset:<sha256>`。`input_assets` 是原工程 source 加该步直接资源操作数去重后的有序引用；后续依赖由祖先前缀传递。读取历史验证引用与操作一致；每次恢复、内存缓存或预览缓存命中均验证前缀所有必需资产的摘要与长度。删除原始外部路径及移动工程不影响重放。失败可能留下未引用资产，manifest 不发布半组。

检查点增加 `PICDOC02` 格式：magic、64 字节 key、u64 JSON 元数据长度、完整 DocumentInfo／预览映射元数据、各 Raster 的 u32 宽高和 RGBA 四个 f32 little-endian 原始位，最后 SHA-256。多层检查点保存渲染结果及**每个独立图层、每个蒙版、变换、名称、显隐、混合、选区和已使用 ID**。恢复检查完整性、维度、目标、映射与预算；即使 key／校验和有效，多层前缀也拒绝扁平 Raster 检查点。丢失或损坏后从较早完整前缀或原资产重建。

旧单画布检查点及无额外映射的画布预览仍使用 `PICFLT01`。渲染缓存身份增加 layers-v1；旧缓存失效只影响速度，不影响工程可读性。内存预算计入全部独立 Raster 与序列化元数据估计，磁盘预算按实际文件字节，读写 scratch 按同时存在的快照和像素准入；沿用缓存锁及淘汰边界，不删除资产。多层状态与渲染缓冲也受 max_buffer_bytes 准入；这仍不是严格进程 RSS 保证。

管线构造限额与 Project 限额取较紧值，资产绑定不得放宽原管线的 max_dimension/max_pixels/max_buffer_bytes/max_input_bytes；外部素材读取、解码和执行均沿用该限制。实际源／图层／蒙版渲染，包括 checkpoint 恢复后的完整合成与指定目标预览，都计入 process_ms。工程首次导入素材读取计入 read_ms，保存计入 write_ms；操作内部已绑定资产的读取／解码随逻辑执行计入 process_ms。缓存读入及拆包仍计入 read_ms。

模板暂只支持原有无外部资产的 canvas 操作。新增图层、蒙版、选区、composite，以及以图层 ID 为目标的原有像素操作，template-export／template-run 均拒绝，返回结构化 `unsupported_template`（非法 target 本身仍可先返回 invalid_target）。不能把旧 layer ID、选区或 mask 随换图模板带入新历史；后续支持前需明确素材和坐标重新绑定规则。

## 验收证据

| 条件 | 测试与证据 |
| --- | --- |
| 顺序、四混合模式、opacity/alpha、无黑边 | `crates/pic-core/tests/layers.rs` 用已知线性像素解析验证四种模式、半透明底／来源和 opacity；透明／被蒙版遮住的蓝色不污染红色边缘；平移、37°旋转、灰 128 和灰度×alpha |
| 稳定 ID、结构化错误 | 同名层重命名／重排／隐藏／删除仍按 ID；禁止复用 ID、非法参照、彩色／错尺寸蒙版、无效变换、选区和多层画布歧义 |
| 跨进程持久化、重放和继续编辑 | `crates/pic-cli/tests/project/layers.rs` 每步独立进程；保存检查点后移动工程、删除原图／图层／蒙版，再调 opacity、undo/redo、清缓存重放、解除蒙版并裁剪独立图层 |
| 完整检查点及权威验证 | `crates/pic-core/src/project/tests/layers.rs` 逐 f32 位比较每层／蒙版；HDR、负值、signed zero、subnormal、隐藏颜色；缓存截断回退、扁平快照拒绝、缓存命中时资产缺失／损坏报错、故障注入保持 manifest |
| 预览／导出与坐标 | 实际 composite 命令、JSON 管线和工程图层四种 blend 的 PNG 解码像素完全一致；旋转区域预览、layer↔canvas↔preview 矩阵、mask 缓存映射、canvas/local 选区并集均回归 |
| 资源／并发／P0 | 大 8×6 项目 + 小构造限额 identity 拒绝；图层／蒙版外部及 asset 引用读取限制；manifest 不变；旧 P0、逻辑 revision、并发锁和完整仓库测试保留 |

验证命令为 README 的 fmt、clippy、workspace tests、release build、diff-check，另运行 release 二进制的 CLI 与 project 集成测试。未增加或升级依赖；Linux 为本轮验证平台。没有承诺图层缩小抗混叠质量、PSD 兼容或大图性能门槛。
