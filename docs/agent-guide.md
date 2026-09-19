# Agent 执行指南与可运行示例

当前版本只执行明确参数的直接编辑，不需要模型或凭据。先用 `capabilities --json` 读取实际操作、参数和限制，再选择命令。`operations` 才是可执行操作；`smart_editing` 为 `not_implemented / roadmap`，目前没有“配置模型即可启用”的能力。未知操作会失败。

以下带 `# pic-verify:` 的命令块是实际验收输入：[verify-package.py](../scripts/verify-package.py) 从本文件提取并按顺序执行，检查 JSON、像素、坐标、历史和失败恢复。每次 `pic` 都启动一个新进程。完整自动运行及安装见 [打包指南](packaging.md)。也可在解包后手动运行：设置 `PIC_PACKAGE_ROOT` 为包的绝对路径，进入一个全新的临时目录，依次执行以下命令块。需要 POSIX sh 和 Python 3 标准库，不需要 jq、系统字体、外部图片或 reference/。

## 准备与发现能力

```sh
# pic-verify: setup
set -eu
PIC_CLI_BIN="$PIC_PACKAGE_ROOT/bin/pic-cli"
"$PIC_PACKAGE_ROOT/libexec/agent_fixtures" generate .
mkdir recipes receipts
cp "$PIC_PACKAGE_ROOT"/examples/*.json recipes/
cp "$PIC_PACKAGE_ROOT/tests/fonts/DejaVuSans.ttf" font.ttf
cp "$PIC_PACKAGE_ROOT/tests/fonts/LICENSE-DejaVu.txt" .
pic() {
  "$PIC_CLI_BIN" --json "$@" 2>cli-stderr.txt || return $?
  test ! -s cli-stderr.txt
}
field() {
  python3 -c 'import json,sys
v=json.load(open(sys.argv[1]))
for key in sys.argv[2].split("."): v=v[key]
print(v)' "$1" "$2"
}
pic --help > receipts/help.json
pic --version > receipts/version.json
pic capabilities > receipts/capabilities.json
pic project apply --help > receipts/apply-help.json
pic project text add --help > receipts/text-help.json
```

生成器创建 192×128 的 `photo.png`、`new-photo.png`、`photo.jpg`、带透明区域的 `background.png`，以及同为 32×24 的主体/灰度蒙版。字体是包内未修改的 DejaVu Sans 2.37，来源、哈希和分发许可见 [字体说明](../tests/fonts/README.md)。测试文字限定 Latin/Greek/Cyrillic 支持范围；不要换成中文后假设能自动选择字体。

顶层 envelope 的 `schema_version=1`，`engine_version=0.1.0`；capabilities 中 `result_schema_version=1`、`pipeline_schema_version=1`，每个操作 `op_version=1`。这些版本分别管理；遇到未知版本停止解释。成功是退出码 0、`ok=true`、`error=null`；失败参见 [错误恢复指南](errors.md)。`--json` 的 stdout 恰好一行 JSON，普通成功/错误的 stderr 为空；警告在 `warnings`。stdout 本身写入失败时只能在 stderr 诊断。

## 普通处理与 JSON 管线

```sh
# pic-verify: ordinary
pic info photo.png > receipts/info.json
pic identity --input photo.png --output copy.png > receipts/identity.json
pic resize --input photo.jpg --width 96 --output resized.jpg --jpeg-quality 90 > receipts/resize.json
pic crop --input photo.png --x 8 --y 8 --width 160 --height 96 --output crop.png > receipts/crop.json
pic rotate --input photo.png --degrees -30 --background '#00000000' --output rotated.png > receipts/rotate.json
pic flip --input photo.png --axis horizontal --output flipped.png > receipts/flip.json
pic canvas --input photo.png --width 224 --height 160 --anchor center --output canvas.png > receipts/canvas.json
pic adjust --input photo.png --exposure 0.5 --saturation 1.1 --output adjusted.png > receipts/adjust.json
pic levels --input photo.png --input-black 0.02 --input-white 0.9 --gamma 1.2 --output levels.png > receipts/levels.json
pic curves --input photo.png --points '[[0,0],[0.5,0.6],[1,1]]' --output curves.png > receipts/curves.json
pic grayscale --input photo.png --output gray.png > receipts/grayscale.json
pic invert --input photo.png --output inverted.png > receipts/invert.json
pic blur --input photo.png --sigma 2 --output blurred.png > receipts/blur.json
pic sharpen --input photo.png --sigma 1 --amount 0.75 --output sharpened.png > receipts/sharpen.json
pic composite --input background.png --overlay subject.png --mask mask.png --x 24 --y 16 --opacity 0.8 --blend screen --output composite.png > receipts/composite.json
pic identity --input background.png --output flattened.jpg --jpeg-background '#ffffff' > receipts/jpeg-alpha.json
pic run --input photo.png --pipeline recipes/photo.json --output direct.png > receipts/run.json
```

[photo.json](../examples/photo.json) 包含两步裁剪和调色，最小输入为 168×104。示例使用 192×128。其结构为：

```json
{"schema_version":1,"operations":[
  {"op":"crop","op_version":1,"target":"canvas","params":{"x":8,"y":8,"width":160,"height":96}},
  {"op":"adjust","op_version":1,"target":"canvas","params":{"exposure":0.5,"brightness":0,"contrast":1,"saturation":1.1}}
]}
```

JSON 参数遵循 capabilities 的 `params`，不自动套 CLI 默认值；未知字段也拒绝。操作按数组顺序执行，每步坐标相对当前画布。工作像素为线性 sRGB RGBA32F；曝光单位 EV，饱和度/对比度为倍率，alpha 为覆盖率。导出才量化，分成多条文件命令会多次量化，不能替代一次管线。参数公式与范围见 [几何/codec](geometry-codecs.md)、[调色/滤镜](adjustments-filters.md)。

CLI 文件路径相对 cwd；JSON 里的素材路径相对**管线文件的规范化父目录**。例如 `recipes/layers.json` 的 `../subject.png` 指向本例 cwd 的主体。输出父目录须存在，已存在文件须显式 `--overwrite`。PNG/JPEG 以外、ICC、高位深、调色板 PNG 等会拒绝；EXIF 主图方向 1–8 在编辑前归一化，导出不保留元数据。透明 JPEG 必须明确铺底颜色。

## P0：观察、坐标映射和跨进程续编

```sh
# pic-verify: project
pic project create --input photo.png --output work.pic > receipts/create.json
initial=$(field receipts/create.json data.revision)
pic project apply work.pic --pipeline recipes/photo.json --expect-revision "$initial" > receipts/apply.json
edited=$(field receipts/apply.json data.revision)
pic project checkpoint work.pic --revision "$edited" > receipts/checkpoint.json
pic project inspect work.pic > receipts/inspect.json
pic project preview work.pic --revision "$edited" --region 8,6,64,32 --width 32 --output region.png > receipts/preview.json
observed=$(field receipts/preview.json data.revision)
pic project export work.pic --output before-followup.png > receipts/before-export.json
pic project apply work.pic --pipeline recipes/followup.json --expect-revision "$observed" > receipts/followup.json
continued=$(field receipts/followup.json data.revision)
pic project export work.pic --output final.png > receipts/final-export.json
pic project undo work.pic --expect-revision "$continued" > receipts/undo.json
undone=$(field receipts/undo.json data.revision)
pic project export work.pic --output undone.png > receipts/undone-export.json
pic project redo work.pic --expect-revision "$undone" > receipts/redo.json
current=$(field receipts/redo.json data.revision)
pic project export work.pic --output redone.png > receipts/redone-export.json
```

本例初始 r0、两步提交 r2、续编 r3；实际程序应像上面一样读返回 ID。一次 apply 是一个撤销组，组内每个逻辑步骤仍可单独 inspect/preview/export。`data.coordinates.preview_to_canvas` 返回本例 `scale=[2,2]`、`offset=[8,6]`，所以预览像素 `(3,4)` 的中心 `(3.5,4.5)` 映射到当前画布 `(15,15)`。反向用 `canvas_to_preview`。区域为半开像素边界；不能把缩略图索引直接用作原图坐标。调用方分析预览并选择后续参数，CLI 不进行图像理解。

分析必须绑定 `(project, revision, target, region, coordinates)`。若观察期间其他写入改变了当前指针，提交会返回 `revision_conflict`，不要只替换 expected revision 重试。重新 inspect、preview、分析坐标，再构造请求。读旧 revision 不移动当前指针；`--expect-revision` 始终比较当前指针。它不是独立单调事务 token：undo/redo 可把指针带回旧 ID。

## 中间步骤续编、参数修订与模板换图

```sh
# pic-verify: history-template
pic project inspect work.pic --revision r1 > receipts/old-inspect.json
pic project preview work.pic --revision r1 --output old-step.png > receipts/old-preview.json
pic project template-export work.pic --revision "$edited" --output recipe.json > receipts/template-export.json
pic project template-run --template recipe.json --bindings recipes/bindings.json --output another.pic > receipts/template-run.json
pic project export another.pic --output another.png > receipts/template-image.json
pic project apply work.pic --revision r1 --pipeline recipes/followup.json --expect-revision "$current" > receipts/branchless-continue.json
current=$(field receipts/branchless-continue.json data.revision)
pic project export work.pic --revision "$continued" --output preserved-old.png > receipts/old-export.json
pic project revise work.pic --step-revision "$current" --params '{"sigma":0.5,"amount":0.5}' --expect-revision "$current" > receipts/revise.json
pic project export work.pic --output revised.png > receipts/revised-export.json
mkdir moved
mv work.pic moved/work.pic
rm photo.png recipes/photo.json recipes/followup.json
pic project cache-clear moved/work.pic > receipts/cache-clear.json
pic project export moved/work.pic --output replayed.png > receipts/replay-export.json
```

这里 `r1` 是本例已返回的第一个 crop 步骤 ID（一般从 `apply.data.steps[].revision` 选取）。旧步骤续编会形成新当前序列、清除原 redo 路径，已提交旧记录仍可读；没有命名分支或合并。`revise` 替换完整 params 并重算后缀，不能原地修改 ops。工程移动、删除外部源图/管线、清派生缓存后仍可恢复，因为 `assets/` 和不可变 `ops/` 才是权威；不要编辑 manifest 或把检查点当作素材。

[bindings.json](../examples/bindings.json) 明确重新绑定新输入 `../new-photo.png`、`canvas` 目标以及每个 `step_N` 的完整参数。本例新 crop 为 `(16,12,128,80)`、曝光为 0.25。`suggested_params` 不填补任何缺失绑定，新工程有独立历史；编号相同不代表身份相同。模板只支持无外部素材的单画布操作，图层树、字体、蒙版、选区和 composite 返回 `unsupported_template`，不会自动迁移内容相关位置。

## 图层、蒙版、文字闭环

```sh
# pic-verify: layers
pic run --input background.png --pipeline recipes/layers.json --output layers-direct.png > receipts/layers-run.json
pic project create --input background.png --output layers.pic > receipts/layers-create.json
base=$(field receipts/layers-create.json data.revision)
pic project apply layers.pic --pipeline recipes/layers.json --expect-revision "$base" > receipts/layers-apply.json
layer_revision=$(field receipts/layers-apply.json data.revision)
pic project preview layers.pic --output layers-before.png > receipts/layers-before.json
pic project preview layers.pic --target subject --output subject-preview.png > receipts/subject-preview.json
pic project preview layers.pic --target mask:subject --output mask-preview.png > receipts/mask-preview.json
pic project checkpoint layers.pic > receipts/layers-checkpoint.json
pic project inspect layers.pic > receipts/layers-inspect.json
font_asset=$(python3 -c 'import json; v=json.load(open("receipts/layers-inspect.json")); print(next(x for x in v["data"]["document"]["layers"] if x["id"]=="title")["kind"]["params"]["font"])')
mv layers.pic moved/layers.pic
rm background.png subject.png mask.png font.ttf recipes/layers.json
pic project text set moved/layers.pic --target title --font "$font_asset" --text 'Office é' --size 24 --line-height 28 --width 128 --height 64 --align right --color '#ffffffff' --expect-revision "$layer_revision" > receipts/text-set.json
pic project preview moved/layers.pic --output layers-preview.png > receipts/layers-preview.json
pic project export moved/layers.pic --output layers-final.png > receipts/layers-export.json
pic project cache-clear moved/layers.pic > receipts/layers-clear.json
pic project export moved/layers.pic --output layers-replayed.png > receipts/layers-replay.json
```

[layers.json](../examples/layers.json) 是可修改的完整模板：稳定 ID `layout` 隔离组、`subject` 主体和 coverage 蒙版、`title` 可编辑文字、`tone` 点调整层及对 `layout` 的显式剪贴。它是 apply/run 管线，不是 `template-run` 支持的换图配方。所有素材和字体路径显式绑定，提交后字体可用 inspect 返回的 `asset:SHA256` 续编。

图层顺序是同级从底到顶。图层本地变换通过返回的六元素仿射矩阵 `[a,b,c,d,e,f]` 映射到画布：`x'=a*x+c*y+e`、`y'=b*x+d*y+f`；预览输出仍使用画布坐标。蒙版按编码灰值 coverage 解读（128 约为 50.2%），不是线性亮度；`mask:subject` 只显示原始蒙版，不是整组最终 alpha。图层模式下 canvas 只支持 identity、无损 crop 和透明 canvas 调整，像素调色指定 raster 图层 ID。文字固定框、LF 换行、LTR 子集、无自动换行或字体回退；不支持中文、bidi、PSD 往返。完整语义见 [图层/蒙版](layers-masks.md)、[组/文字/调整层](groups-text-adjustments.md)。

另外，包内 `libexec/layer_milestone` 复用已有 Rust 里程碑，独立验证蒙版透明/半透/不透明像素、文字覆盖、移动工程后改字、undo/redo、字体丢失、冷重放和可选 4K 双层。运行方式与产物见 [打包指南](packaging.md)。
