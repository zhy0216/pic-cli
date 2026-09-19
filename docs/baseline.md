# 首轮开发与小图基线

记录日期：2026-09-19 UTC。对应任务 01、pic-cli 0.1.0 及本仓库 `Cargo.lock`。这是当前工作机器上的启动与 codec 基线，尚不是照片级编辑性能验收。

## 环境

| 项目 | 实际观察 |
| --- | --- |
| OS / 架构 | Ubuntu 24.04 LTS，Linux 6.8.0-31-generic，x86_64-unknown-linux-gnu |
| CPU | AMD EPYC Processor（虚拟型号），KVM；1 socket、4 core、每核 2 thread，共 8 个可见逻辑 CPU；`nproc=8` |
| 内存 | `/proc/meminfo` MemTotal=16,367,340 KiB（约 15.6 GiB），无 swap |
| cgroup | 当前 session 的 memory.max=`max`，cpu.max=`max 100000`；未观察到额外 session 配额 |
| 仓库磁盘 | `/dev/vdb`，110 GiB 虚拟块设备、ext4，挂载 `/home/ubuntu`；设备报告 ROTA=1，不能据此确定底层物理介质；首次观察约 7 GiB 可用 |
| 临时素材 | `tempfile::tempdir()` 默认系统临时目录；基线未强制冷热磁盘缓存，也未清空 OS page cache |
| Rust | rustc 1.98.1 (48a229cea 2026-09-01)，LLVM 22.1.8 |
| Cargo | cargo 1.98.1 (797e8a9bc 2026-08-05) |
| 构建 | `cargo build --release`，标准 release 配置；未设置 RUSTFLAGS、CARGO_ENCODED_RUSTFLAGS、CARGO_BUILD_TARGET、CARGO_BUILD_JOBS；未添加 native CPU 编译选项 |
| codec | image 0.25.10，仅启用 png/jpeg；image 依赖的 PNG/JPEG codec 版本由 Cargo.lock 固定 |
| 并发 | 基线顺序启动单个 CLI 进程；核心没有 Rayon 线程池，也没有批量并发或模型进程 |

Cargo 清单声明 Rust 1.88 下限，与所选 image 的要求对齐；当前只实际验证了 1.98.1，没有验证较老工具链、macOS、Windows 或其他文件系统。

## 复现方式与素材

```sh
cargo build --release
cargo run --release -p pic-cli --example foundation_baseline -- "$PWD/target/release/pic-cli"
/usr/bin/time -f 'version_max_rss_kib=%M' target/release/pic-cli --version
```

[基线程序](../crates/pic-cli/examples/foundation_baseline.rs) 在临时目录生成 64×48 RGBA8 PNG（RGB 梯度，含透明度）及 64×48 均匀 RGB JPEG `[64,128,192]`（输入 quality=100）。每组先预热一次，再启动 30 个独立 CLI 进程；复用已热的文件缓存，管线为 `{"schema_version":1,"operations":[]}`。输出覆盖同一临时目标，JPEG 导出 quality=90，PNG 使用默认编码设置。样例、管线和输出退出后自动清理。

wall time 从父进程开始 spawn 到回收子进程和 stdout/stderr 完毕，含启动、参数解析、编解码、发布、输出关闭和进程退出，不含 fixture 生成或 Cargo 编译。每次 JSON 的版本、成功状态和尺寸均有断言；最终输出重新解码，PNG 要求精确 RGBA，JPEG 各通道相对输入解码值误差不大于 3。此 JPEG 素材极易压缩，不能代表照片质量或照片级吞吐。

p50/p95 使用 30 个样本排序后的 nearest-rank（第 15、29 个），单位毫秒。各阶段分位数独立计算，不能相加得到 total 的分位数。

## 实测结果

| 新进程调用 | wall p50 ms | wall p95 ms |
| --- | ---: | ---: |
| `--version` | 1.567 | 1.986 |
| `info input.png --json` | 1.775 | 2.261 |
| `info input.jpg --json` | 1.874 | 2.518 |
| PNG → 空管线 → PNG，`--overwrite --json` | 2.059 | 2.575 |
| JPEG → 空管线 → JPEG q90，`--overwrite --json` | 2.261 | 3.173 |

主要阶段的 p50 / p95（毫秒）：

| 阶段 | PNG 空管线 | JPEG 空管线 |
| --- | ---: | ---: |
| validation | 0.045847 / 0.071224 | 0.049003 / 0.068179 |
| read | 0.013014 / 0.021921 | 0.014136 / 0.017443 |
| decode（含工作像素转换） | 0.213079 / 0.276086 | 0.259595 / 0.348732 |
| process（空循环，无像素操作） | 0.000070 / 0.000240 | 0.000070 / 0.000230 |
| encode | 0.154620 / 0.238567 | 0.205355 / 0.282018 |
| write（含发布和关闭） | 0.119954 / 0.220943 | 0.145231 / 0.227827 |
| CLI 内部 total（不含进程启动/结果写出/退出） | 0.692907 / 0.879946 | 0.799756 / 1.136938 |

`info` 的 decode p50/p95：PNG 0.215555/0.266058 ms，JPEG 0.249947/0.409285 ms；没有 process/encode/write 阶段。`--version` 另用 `/usr/bin/time` 单次观察峰值 RSS 为 **3,200 KiB**；这不是 30 次分位数，也不是图像处理峰值内存测量。

## 剩余未知事项

尚未测量冷文件缓存、真实照片/1080p/4K/高像素输入的端到端耗时和峰值内存、几何/调色/滤镜、图层、工程冷重放/检查点/预览，以及长管线的性能。资源预算目前是准入估算，不能等同全进程 RSS 上限。底层物理磁盘、宿主争用和其他进程影响没有隔离。

JPEG 色彩管理、EXIF 自动方向、元数据保留、其他格式/位深、具体 Photoshop 参数/输出兼容程度和跨平台发布行为仍需后续任务确定与验证。未承诺比 Photoshop、libvips 或其他 CLI 更快，也未把这些小图数字作为产品 SLO。智能编辑仅为未来 roadmap，不属于本轮选型、测量或交付要求。
