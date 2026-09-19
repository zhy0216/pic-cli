# pic-cli

面向 AI agent 的 Rust 图像编辑 CLI。调用方提供明确的操作和参数；本轮产品目标是普通像素编辑、图层合成、文字，以及基于源素材和不可变 ops 的工程历史与预览，不需要 LLM 或模型参与。智能抠图、分割、修复、生成填充/扩图仅列为未来 roadmap。

当前 **0.1.0** 已实现 PNG/JPEG 信息查询、EXIF 方向归一化、编解码参数、几何编辑，以及曝光 EV、亮度、对比度、饱和度、色阶、曲线、灰度、反相、高斯模糊和锐化的单命令与有序 JSON 管线。图层、文字、工程持久化与预览仍未实现。请以 `capabilities` 为机器可读的实际支持列表。

## 构建与使用

首轮验证平台为本机 Linux x86_64；其他平台尚未验证。依赖锁定在 `Cargo.lock`，`image` 仅开启 PNG/JPEG codec。编译器与基线环境见 [基准记录](docs/baseline.md)。

```sh
cargo build --release
target/release/pic-cli --help
target/release/pic-cli --version
target/release/pic-cli capabilities --json
target/release/pic-cli info photo.png --json
target/release/pic-cli identity --input photo.png --output copy.png --json
target/release/pic-cli resize --input photo.jpg --width 1600 --output resized.jpg --jpeg-quality 90 --json
target/release/pic-cli crop --input photo.png --x 100 --y 80 --width 640 --height 480 --output crop.png --json
target/release/pic-cli rotate --input photo.png --degrees -30 --background '#00000000' --output rotated.png --json
target/release/pic-cli flip --input photo.png --axis horizontal --output flipped.png --json
target/release/pic-cli canvas --input photo.png --width 1920 --height 1080 --anchor center --output canvas.png --json
target/release/pic-cli adjust --input photo.png --exposure 0.5 --saturation 1.1 --output adjusted.png --json
target/release/pic-cli curves --input photo.png --points '[[0,0],[0.5,0.6],[1,1]]' --output curves.png --json
target/release/pic-cli blur --input photo.png --sigma 2 --output blurred.png --json
target/release/pic-cli sharpen --input photo.png --sigma 1 --amount 0.75 --output sharpened.png --json
```

空管线仍会完整读取、解码、转换到工作像素、编码并原子发布输出；不保证压缩文件字节相同。生成临时示例：

```sh
pic_example_dir=$(mktemp -d)
cat > "$pic_example_dir/pipeline.json" <<'JSON'
{"schema_version":1,"operations":[]}
JSON
target/release/pic-cli run --input photo.png \
  --pipeline "$pic_example_dir/pipeline.json" \
  --output "$pic_example_dir/result.png" --json
```

单操作使用同一核心入口；等价的 identity 管线为：

```json
{
  "schema_version": 1,
  "operations": [
    { "op": "identity", "op_version": 1, "target": "canvas", "params": {} }
  ]
}
```

输出默认根据 `.png` / `.jpg` / `.jpeg` 后缀选格式，`--format png|jpeg` 可显式指定；JPEG 支持 `--jpeg-quality 1..100`（默认 90），PNG 支持 `--png-compression 0..9`（默认 6）。透明图导出 JPEG 默认报错；显式 `--jpeg-background '#ffffff'` 在线性光下铺底。覆盖现有文件必须加 `--overwrite`。失败保留已有输出。完整参数、采样公式、EXIF/ICC 范围及多步示例见 [几何与编解码契约](docs/geometry-codecs.md)，基础资源/发布规则见 [执行契约](docs/foundation-contract.md)。

调色在线性 RGB 上计算，步骤之间保留 RGBA32F 的负值、高亮和透明度，最终导出才量化。`adjust` 按曝光→亮度→对比度→饱和度执行；`levels` / `curves` 支持 RGB 或单个色彩通道；JSON 参数显式必填。范围、默认值、色阶/曲线外延、模糊边界与 alpha 语义及管线示例见 [调色与滤镜契约](docs/adjustments-filters.md)。

`--json` 在子命令前后均可使用，stdout 只输出一个版本化 JSON 对象，包括帮助、版本和错误；其他诊断走 stderr。不加 `--json` 时数据命令输出缩进 JSON，help/version 输出文本。退出码：成功 0，执行错误 1，命令行解析错误 2。

## 工程布局

```text
Cargo.toml / Cargo.lock        workspace 与唯一依赖锁
crates/pic-core/src/
  codec.rs, codec/             格式准入、读写、像素转换、原子发布
  operation.rs, operation/    操作规格、参数校验、几何/调色/滤镜与统一执行接口
  pipeline.rs                 有序执行、资源解析、文件处理入口
  document.rs                 浮点像素状态、稳定目标与版本契约
  limits.rs                   资源准入限制
  result.rs, error.rs          版本化结果、错误、警告、阶段计时
  capabilities.rs             实际能力清单
crates/pic-core/tests/         核心精度、校验与发布测试
crates/pic-cli/src/main.rs     clap 参数解析、JSON/文本输出、退出码
crates/pic-cli/tests/          真实进程、文件与像素集成测试
crates/pic-cli/examples/       可复现的小图启动/codec 基线
docs/                         契约、功能矩阵、环境和验收证据
plans/fast-image-editing/      方案与任务队列
reference/                    忽略的只读上游参考，不参与构建
target/                       忽略的构建产物
```

未来操作扩展 `operation`，沿用 `pipeline::run`；工程历史在 `document` 基础上增加独立持久化模块。PNG/JPEG 只用于输入/导出，不能作为浮点工作状态的无损检查点。详细边界见 [执行与版本契约](docs/foundation-contract.md)，当前与未来能力见 [功能矩阵](docs/capability-matrix.md)。

## 校验

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
git diff --check
PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli
cargo run --release -p pic-cli --example foundation_baseline -- "$PWD/target/release/pic-cli"
```

测试与基线在临时目录自生成素材、管线及输出，不把样例或测量产物写入仓库。验收范围、测试对应关系见 [基础验收记录](docs/foundation-validation.md)。
