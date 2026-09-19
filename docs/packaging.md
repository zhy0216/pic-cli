# 构建、安装与独立打包验收

已验证：Ubuntu 24.04、Linux 6.8 x86_64、glibc 2.39，rustc/cargo **1.98.1**。`Cargo.toml` 声明 `rust-version=1.88`，本轮没有测试该最低工具链。macOS、Windows、Linux ARM、musl、较旧 glibc 及其他文件系统未验证；工程存储使用 POSIX 目录句柄/文件锁，不能把 Rust 的跨平台能力当作本产品的平台验收。

构建只需 Rust/Cargo 和锁定依赖，未缓存依赖时 Cargo 需要访问注册表；无原生图像库、模型、GPU 或系统字体依赖。运行 `bin/pic-cli` 不需要 Rust、Python、源码或 reference/，但需要本包构建平台的系统动态库。包内 `runtime-libraries.txt` 记录本次实际 `ldd`；这是 host GNU/Linux 构建，不是静态或通用 Linux 发布包。

## 源码构建与本地安装

在任意完整 checkout（包括 main checkout）根目录运行：

```sh
cargo build --locked --release
target/release/pic-cli --version
target/release/pic-cli capabilities --json
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/pic-cli "$HOME/.local/bin/pic-cli"
"$HOME/.local/bin/pic-cli" --json --version
```

最后两条将当前二进制安装到个人目录；可选择其他安装路径。也可用 `cargo install --locked --path crates/pic-cli --root /your/install/prefix` 从本 checkout 安装。本任务实际验证了 release 构建、二进制复制安装和独立运行；没有测试 crates.io 发布或这个替代 cargo install 命令，不宣称已有公开下载/注册表包。仓库未声明产品分发许可证，不能从依赖许可证推导产品许可。

## 一条命令打包并验收

打包脚本使用 Python **3.12+ 标准库**（本机实测 3.14.7）、sh、`install`、Linux `ldd`、Git、Rust/Cargo，无 pip 包或 jq。它显式构建 rustc 的 host target，保留 Cargo.lock，失败立即非零退出。默认包含普通 4K 双层验收，建议有约 1 GiB 可用运行内存及数 GiB 构建空间；这不是 CLI 的硬 RSS 保证。

```sh
python3 scripts/package.py
```

可从其他 cwd 使用脚本的绝对路径；它从自身位置找到 checkout，不修改任何 Git 分支。`--output /absolute/new-parent` 可更换生成物父目录。路径解析父目录符号链接后，若等于或位于待复制的 `docs/`、`examples/`、`tests/fonts/`、`plans/fast-image-editing/roadmap/` 内，会在创建目录和构建前拒绝，避免复制输出自身。默认 `target/dist/` 和外部 `/tmp` 均可用。每次创建新目录，保留旧包；不会清理其他任务产物。默认生成物：

```text
target/dist/pic-cli-<随机后缀>/
  pic-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz
  pic-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
  pic-cli-0.1.0-x86_64-unknown-linux-gnu/
    bin/pic-cli
    libexec/agent_fixtures, layer_milestone
    docs/, examples/, tests/fonts/, scripts/verify-package.py
    plans/fast-image-editing/roadmap/
    README.md, Cargo.toml, Cargo.lock, DEPENDENCIES.json
    build-info.json, runtime-libraries.txt, SHA256SUMS
  build.log
  verification/                 实际执行目录移回后保留的完整证据
    verification.json           仅全部成功时 ok=true
    guide.sh, guide.stdout, guide.stderr, receipts/, calls.jsonl
    moved/*.pic, *.png, layer-milestone/report.json
  package-result.json          仅打包及独立验收成功后生成
```

脚本先创建 tar.gz，再将**该压缩包**解包到系统临时目录，从另一个全新临时 cwd 执行其中的二进制与指南。删除外部原图/蒙版/字体、移动工程后重放；不调用 Cargo、不读取 reference/ 或源码工作目录。构建两个验收辅助程序时将字体/许可编入已有 layer_milestone；运行时不依赖编译路径。脚本将临时执行目录移动到 verification/ 保留工程、图片和日志；其中绝对路径记录的是当时的临时目录，当前证据按保留目录内的相对路径读取。

`build-info.json` 记录源码 commit、dirty 状态、host、编译器、构建命令、Cargo.lock 摘要和编译环境覆盖；不声称不同工具链/路径构建出相同字节。`SHA256SUMS` 覆盖包内全部普通文件（自身除外），外层 `.sha256` 校验压缩包；这些是完整性校验，不是发行签名。`DEPENDENCIES.json` 记录锁定依赖的许可证声明/来源；测试字体完整许可随包分发。文档中指向 crates/ 的源码链接用于完整 checkout，本二进制验收包不含源码 workspace。

## 单独重跑已解包的包

无需 Cargo，`--output` 必须不存在；用实际解包路径替换下例路径：

```sh
python3 /absolute/package/scripts/verify-package.py /absolute/package \
  --output /tmp/pic-package-recheck --4k
```

只有指南/小图闭环时可省略 `--4k`；正式 `package.py` 始终运行 4K 检查。验证器读取包内指南带标记的全部命令块：普通几何/调色/滤镜/合成、P0 预览坐标/expected revision/续编、undo/redo/旧步骤修订、模板换图、图层/蒙版/组/文字/调整/clip。之后检查所有 capabilities 命令的 JSON help、错误码/退出码/stderr、失败无发布，以及解码像素一致性。缺字、缺字体和不支持的图层模板都应明确失败。

单独运行已有图层里程碑（输出目录须不存在）：

```sh
/absolute/package/libexec/layer_milestone /absolute/package/bin/pic-cli \
  /tmp/pic-layer-recheck --4k
```

这些是正确性验收，不能当作新的性能基准。30 轮 release 性能、4K 调整仍超过建议 1 秒的限制、缓存收益和原始数据见 [当前性能报告](performance.md)；任务 01 的小图数据保留于 [历史基线](baseline.md)。

## 完整仓库校验

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
git diff --check
python3 scripts/package.py
```

构建/测试记录与当前交付证据见 [任务 12 验收记录](packaging-validation.md)。只有直接编辑在本轮交付；智能抠图/分割/修复/填充/扩图明确保留在 [roadmap](../plans/fast-image-editing/roadmap/README.md)，目前没有可配置的模型后端。
