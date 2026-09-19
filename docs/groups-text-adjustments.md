# 分组、剪贴、文字与调整层

任务 07 在现有 `OperationSpec → Pipeline → Document → Project` 上增加 7 个语义操作；一次性 `run`、工程提交、历史重放、预览和导出共用它们。原始素材和不可变 ops 仍是权威状态；字体同图片和蒙版一样通过 SHA-256 进入 `assets/`。没有 LLM、图像模型或平行工程状态。

## 图层树和作用域

`inspect.document.layers` 继续是稳定 ID 的有序数组。每项增加 `parent`、`clip` 和 `kind`；`kind.type` 是 `raster/group/text/adjustment`，后两种的可编辑参数在 `kind.params`。同一 parent 的项按数组相对顺序从底向顶合成；parent=null 是根。子项的数组位置不决定组在根中的位置，组始终作为根/父组的一个整体参与合成。

组使用**隔离合成**：在明确 `width×height` 的本地透明画布上合成子项，超出组边界的内容仅在渲染时裁切，再将整个组按组蒙版、变换、剪贴、opacity 和 blend 合成到父作用域。子项本地像素保留可编辑。组 opacity 只在最终组面上应用一次，不分发给子项；组内混合和调整不读取组外背景。暂不提供 pass-through 组、自动适配组边界或组尺寸修改。

`layer_parent` 保留局部变换，坐标从新 parent 的本地空间解释，不隐式保持世界位置。移动一个组会连同其子树移动。`layer_reorder` 的 before 必须在同一 parent 内；null 移到当前作用域最顶。删除有子项的组或仍被 clip 引用的目标会失败，调用方须先重设依赖。重新排序、重设 parent 也必须保持所有 clip 依赖有效。每个逻辑步骤都验证结构，不能靠同一批次的稍后步骤补救暂时非法的状态。

父引用必须指向组。组包含关系与 clip 依赖组成的图采用无递归的拓扑校验，缺失引用返回 `invalid_target`，环返回 `dependency_cycle`，非法层级/顺序/类型返回 `invalid_hierarchy`。最多 32 个祖先组；超出返回 `resource_limit`。渲染递归只发生在完整验证之后。所有本地变换仍遵守任务 06 的范围；组合矩阵还须具有有限非零行列式、有限逆矩阵和可用的线性往返精度。27 层各放大 1e6 的组合在行列式溢出前被拒绝，整组不会发布 revision。旧单画布 fast path 只允许 root raster、无 clip 的原有状态。

组与子项的世界矩阵按 `parent_world × child_local` 合成。inspect/preview 和选区都使用这个矩阵。canvas crop/padding 只平移根项及 canvas 选区，避免重复平移后代；组尺寸保持不变。

## 剪贴和非破坏调整

`layer_clip {"base":"ID"}` 把当前项的 alpha 乘以下方同级 base 的**独立有效 alpha**。base 可为 raster、text 或 group，必须先于当前项；adjustment 没有独立图像 alpha，不能作为 base。覆盖率包括 base 的局部蒙版、变换、显隐、opacity 以及它自身的 clip 链，采样发生在当前 parent 的像素空间。base 仍照常参与可见合成；不以累计背景 alpha 替代 base alpha。`base:null` 解除剪贴。源蒙版在预乘采样前生效；剪贴覆盖率在两者映射到 parent 后相乘。

调整层支持 `adjust/levels/curves/grayscale/invert`，使用现有 v1 线性 RGBA32F 点操作公式。它只读取**所在作用域中、位于自身下方的累计合成结果**。上方项不受影响，组内调整不改变组外背景，下方原始图层像素不被修改。暂不提供 blur/sharpen 等空间滤镜调整层。

调整层创建时的 width/height 是本地覆盖矩形，初始 coverage=1；可绑定同尺寸蒙版、变换及 clip。令旧背景为 B、点调整为 A(B)、有效覆盖率乘 opacity 为 w，则 RGB=`B*(1-w)+A(B)*w`，alpha 保留 B 的原始位；w=0 或 1 使用原值或调整结果。全透明背景不会变成不透明。只接受 normal blend，其他模式准确拒绝。

解析样例：红 `[1,0,0,0.5]` 位于组内，invert 调整层 opacity=0.5、蒙版 coverage=0.5，组内结果是 `[0.75,0.25,0.25,0.5]`；再与组外背景正常合成。两个完全重叠的不透明子层（红下蓝上）的组 opacity=0.5，在绿色不透明背景上得到 `[0,0.5,0.5,1]`，不是对子层分别减半后的结果。测试使用浮点预期值核对。

## 字体与真实排版范围

必须传入明确字体文件或已有 `asset:<sha256>`，没有系统字体查找、默认字体或 fallback。支持**静态、单 face、TrueType glyf 轮廓**；字体集合、可变字体、CFF、彩色/位图字体不在当前范围。缺失外部字体返回 `file_not_found`，无效/不支持的字体返回 `invalid_font`；工程字体资产缺失/损坏返回 `asset_missing/integrity_mismatch`，即使有文字像素检查点或预览缓存也会校验它。

[Rustybuzz 0.20.1](https://docs.rs/rustybuzz/0.20.1/rustybuzz/) 执行默认 OpenType shaping，使用 GSUB/GPOS、连字、kerning 和 mark 定位；[ab_glyph 0.2.32](https://docs.rs/ab_glyph/0.2.32/ab_glyph/trait.Font.html) 根据字形 ID、浮点位置和轮廓生成抗锯齿 coverage。工作颜色为线性 sRGB，coverage 乘文字颜色 alpha，再按 source-over 合成。字体文件摘要、文本参数、引擎及库版本进入重放/缓存身份。

当前排版支持水平 LTR，**每一行使用 Latin、Greek、Cyrillic 三种字母表之一**，可附 Common/Inherited 字符（如标点、数字、组合附加符号），还须在显式字体中存在字形。每行混合多种字母表、CJK、阿拉伯等复杂脚本、双向文字、竖排、变体选择和控制字符不受支持。文本仅用 LF 分行，CR/CRLF/tab 明确拒绝；暂无自动换行、字体回退、自动缩小、两端对齐。CJK 若字体中缺字报告具体 `missing_glyph U+....`，即使另一个字体包含该字也会因未支持脚本而拒绝，不能据此宣称中文支持。

实测覆盖：`Café ffi`、`e\u0301`（与预组合 é 渲染一致）、`Ωμέγα`、`Привет`。`ffi` 实际形成一个连字字形，`AV` 的 shaping advance 小于分别排版之和。测试还逐像素验证扩大居中框后的平移覆盖率、真实三行文字以及抗锯齿 alpha。可分发测试字体及许可见 [tests/fonts](../tests/fonts/README.md)。

`TextParams` 全部字段明确：

| 字段 | 语义 |
| --- | --- |
| text | UTF-8，最多 65536 字节；保留显式 LF 和空行 |
| font | 明确文件或资产引用；提交后规范化为内容摘要资产 |
| size | 1..512 像素/em，与 DPI 无关 |
| line_height | 1..4096 像素，基线间距；CLI 默认 1.2×size，JSON 必填 |
| width / height | 固定本地文本框；溢出内容在渲染时裁切，无自动换行 |
| align | left / center / right，按整行 shaping advance 对齐，包括空格 |
| color | `[r,g,b,a]`，每项 0..255 的 sRGB 颜色；CLI 支持 `#RRGGBB[AA]` |

首条基线=`font.ascender * size / units_per_em`，第 n 行加 `n*line_height`，使用 shaping 的字形 offset/advance。位置在文字图层的 `layer_transform` 中设置。`text_set` 完整替换 TextParams 并重新塑形，保留图层变换、parent、clip、显示属性和兼容尺寸的蒙版；若新框尺寸与旧蒙版不符，必须先移除蒙版。文字、group 和 adjustment 拒绝破坏性本地像素编辑，避免把可编辑参数与像素变成两套状态。

## CLI 与 JSON

```sh
pic-cli project group add work.pic --id layout --width 1920 --height 1080 --expect-revision r0
pic-cli project layer parent work.pic --target base --parent layout --expect-revision r1
pic-cli project text add work.pic --id title --font tests/fonts/DejaVuSans.ttf \
  --text 'Café ffi' --size 48 --width 600 --height 100 --align center --color '#ffffffff' --expect-revision r2
pic-cli project layer parent work.pic --target title --parent layout --expect-revision r3
pic-cli project adjustment add work.pic --id tone --width 1920 --height 1080 \
  --op adjust --params '{"exposure":0.5,"brightness":0,"contrast":1,"saturation":1}' --expect-revision r4
pic-cli project layer clip work.pic --target tone --base layout --expect-revision r5
```

修订须使用实际返回的 revision。所有编辑命令仍走同一 `Project::apply`，支持 `--expect-revision`、`--revision`、整组提交和撤销/重做。`group` 的 set/transform/reorder/remove 使用现有 layer 命令。`project text set` 完整提供文字字段；移动工程后可复用 inspect 中的 font 资产引用。`project adjustment set` 替换点操作与参数。

| 新 op（均为 op_version=1） | target | params |
| --- | --- | --- |
| group_add | canvas | `{id,name,width,height}` |
| layer_parent | layer ID | `{parent: group ID或null, before: sibling ID或null}`；省略同 null |
| layer_clip | layer ID | `{base: lower sibling ID或null}`；省略同 null |
| text_add | canvas | `{id,name,text: TextParams}` |
| text_set | text ID | `TextParams` |
| adjustment_add | canvas | `{id,name,width,height,adjustment:{op,params}}` |
| adjustment_set | adjustment ID | `{op,params}`，固定复用该点操作 v1 |

精确参数清单与支持状态在 `capabilities --json` 中。`run` 可使用完全相同的 JSON 管线生成一次性图片。模板尚无图层树、字体、clip、调整目标的重绑定规则，因此 template-export/template-run 准确拒绝上述操作，不携带原工程字体/图层内容进入新工程。

## 预览、检查点和资源准入

canvas 预览与 export 使用同一合成结果；区域及分辨率只影响观察输出。group 预览显示整个隔离组。raster/text 单层预览包含自身 clip 依赖和祖先的边界、变换、蒙版、显隐与 opacity，不包含其他兄弟的颜色；adjustment 预览显示作用域中截至该调整层的已调整前缀，再应用祖先。其 JSON 映射仍是目标本地坐标到画布/预览的真实仿射矩阵。

`mask:ID` 显示原始蒙版覆盖率经组合世界矩阵采样后的灰度，忽略自身/祖先显隐、opacity、clip 和其他蒙版，也不采用祖先组边界裁切；用于独立检查蒙版本身，不能把它误读为整组的最终有效 alpha。

`PICDOC03` 检查点保存完整 DocumentInfo、新 kind/parent/clip 参数、文字可重建的渲染像素，以及每层/蒙版的每个 f32 位。group 的透明本地底面和 adjustment 的覆盖矩形由 ops 求值产生，检查点不成为可修改的权威数据。渲染 key 升为 layers-v2，包含隔离组、剪贴、调整作用域和文字库版本；旧 PICDOC02 不复用，按 ops 安全重建；单 raster PICFLT01 格式保留。旧工程 manifest/ops 不需要改写。

`max_buffer_bytes` 分开计算文档像素与作用域峰值，保留明确限额，**不是进程 RSS 保证**。令当前作用域面积 S、唯一剪贴 base 数 K：普通合成临时面为 `48*S`（back/front/output），调整为 `64*S`（另保留 adjusted back），剪贴 alpha 最多 `4*S*K`。子组递归期间只保留父 back 和已有覆盖率；返回后保留一个子组结果面用于父合成。峰值取这些阶段的最大值，最后加全部原始图层/蒙版像素一次。未把每份原始像素都乘四，也没有删除预算校验。字体/codec 的内部 scratch 和调用方自身已保留的数据不是严格 RSS 承诺。

两张普通 3840×2160 RGBA32F 图层的准入为 `80*3840*2160 = 663552000` 字节，低于默认 1 GiB。小尺寸回归用两张 8×4 图层，在 2560 字节准入通过、2559 字节拒绝；clip 的额外 128 字节及 group/adjustment 递归峰值也分别测试。一般 API 调用方可通过 `ResourceLimits.max_buffer_bytes` 指定限额，CLI 仍使用统一默认限额。

## 可复用验收

```sh
cargo build --release
cargo run --release -p pic-cli --example layer_milestone -- \
  "$PWD/target/release/pic-cli" /tmp/pic-layer-milestone-07 --4k
```

输出目录必须不存在。每一步启动新的真实 CLI 进程。输出包括前后 direct/preview/export PNG、移动后的可编辑工程、字体许可、`evidence.json` 和 `report.json`；加 `--4k` 执行普通 4K 双层的一次性及工程导出并比较像素。不把大图分配加入普通测试。以下各项均有对应测试：

| 验收 | 证据 |
| --- | --- |
| 分组/clip/adjustment 语义 | `pic-core/tests/groups_text.rs` 的解析像素、隔离组 opacity/顺序/显隐/边界、嵌套坐标/裁切/选区、独立 clip alpha/链、组作为 clip 双方、五种点调整与原核心一致、作用域和 alpha |
| 非法依赖/数值边界 | 层级、缺失目标、自引用/双向 clip 环、forward/cross-scope clip、删除依赖、generated 像素编辑拒绝；27 层极端缩放整组无发布、正常旋转缩放往返；legacy 新字段拒绝 |
| 真实排版 | `ffi` 字形数、`AV` 字距、组合 é 与预组合像素一致、三种字母表、换行/对齐/溢出及 coverage；缺字 U+4E2D、字体丢失、不支持脚本/控制字符准确报错 |
| 完整工程 | `project/tests/groups_text.rs` 每层/蒙版 f32 位与参数比较；移动字体和工程、改 text/adjustment/group、undo/redo、冷重放、旧快照回退；字体 missing/corrupt 即使缓存命中仍失败 |
| CLI 里程碑 | `pic-cli/tests/project/typography.rs` 复用上述示例；背景+主体+外部蒙版+文字，经 r9→保存/移动→r10 修改→undo/redo→导出；每个预览/直接处理/导出解码像素相同，尺寸/位置/透明度和 2 commits / 10 steps 核对 |
| 帮助与模板 | 新子命令真实执行、help、30 个能力操作、失败不改变 manifest、stale revision 拒绝；新操作模板拒绝 |

完整校验及本机 release 实测结果记录在当前 todo 的归档验收记录中。本轮不承诺中文/全 Unicode、Photoshop 参数或 PSD 兼容、自动换行、大图性能门槛或其他平台。
