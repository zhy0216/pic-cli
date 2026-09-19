# 普通编辑与工程端到端性能

本记录对应 task 11，Linux x86_64、固定 release 构建和无需模型的直接编辑。原始逐次结果在 [performance/](performance/)，完整汇总可由脚本重算。这里的结果只描述这组固定素材与当前虚拟机，不构成其他硬件、真实照片集或后端的速度承诺。

## 运行条件

| 项目 | 本次实测条件 |
| --- | --- |
| 系统 | Ubuntu 24.04 LTS，Linux 6.8.0-31-generic，glibc 2.39，x86_64 |
| CPU | KVM 虚拟 AMD EPYC，4 核/8 个可见逻辑 CPU；前后采样均固定 affinity=[0] |
| 内存 | MemTotal=16,367,340 KiB（15.6 GiB）；资源限制是核心准入预算，非 RSS 硬上限 |
| 磁盘 | `/home/ubuntu`，同一 `/dev/vdb` ext4，110 GiB 虚拟磁盘；ROTA=1 不足以判断底层物理介质 |
| 采样器 | Python 3.14.7，GNU time；Python 最低要求 3.11 |
| 编译器 | rustc 1.98.1 / LLVM 22.1.8，cargo 1.98.1；Cargo.lock 不变 |
| release | 标准 release profile，未设置 RUSTFLAGS/native CPU/LTO/codegen 环境覆盖，未升级依赖/增加 feature |
| 并发 | 一次一个 CLI；无新增线程池；独立的 4K 运行每 5 ms 观察到的最大线程数为 1，见 resource-check.json |
| OS 缓存 | 未 drop_caches，预热与素材/工程复制使其偏热；不是冷磁盘基准 |
| libvips | `vips --version` 返回命令不存在；没有实测 libvips 对照，不引入产品后端 |

完整 lscpu、MemInfo、mount/lsblk、工具链、环境变量和测量前后负载在 meta 中；固定 cgroup v2 路径读不到 cpu.max/memory.max，不能据此认定无宿主配额。虚拟机宿主频率、物理磁盘及其他进程争用未隔离。采样期间没有本任务的编译或测试；原始 load/switch/page-fault 数据保留所有干扰。空间不足风险通过删除本任务的可重建试跑目录和 debug 构建缓存控制，前后基准数据始终在同一文件系统。

## 测量方法

[生成器](../crates/pic-cli/examples/performance_fixture.rs) 用确定的整数坐标函数生成梯度、锐利方格、噪声细节和全范围 alpha。素材包括 1920×1080、3840×2160 JPEG q95 与 RGBA PNG，生成器、文件 SHA-256、字节数及完整操作参数记录在每组 `.meta.json`。4K 图层是完整 3840×2160 背景和同尺寸透明前景，screen 混合、opacity=0.7。没有小 overlay 替代完整图层。

[采样脚本](../scripts/performance.py) 每场景预热一次，之后至少 30 次成功的新进程。每一轮用固定 seed=11 打乱 16 个场景顺序，逐一执行；进程和采样器绑定到逻辑 CPU 0。p50/p95 为 nearest-rank，第 15/29 个排序样本；阶段分位数独立计算，不可相加。没有删掉慢样本。

外部 wall 从启动 GNU time 包装器前到进程退出，包含命令启动、读写、编解码、文件关闭以及包装器开销；不包含素材生成、工程复位、JSON 解析、输出核对和磁盘统计。JSON 的 `total_ms` 不含完整进程生命周期；`--version` 是纯文本，没有阶段结构。输出使用既有原子发布规则，不额外 fsync。

峰值 RSS 来自 GNU time 对**本次 CLI 子进程**的 `wait4`，单位 KiB；不使用累计 `RUSAGE_CHILDREN`。试跑发现直接由 Python fork/exec 后取 wait4 会把 Python 继承内存峰值计入小进程，因而正式样本全部采用 GNU time。原始数据还包含本次子进程 CPU 时间、page faults、块 I/O、上下文切换及测量前后 load average。

OS 页缓存不清理，也不伪称冷缓存。所有场景经过预热，copytree、资产核对和输出核对也可能使页缓存变热。`project_replay` / `layers4k_replay` / `layers4k_checkpoint_store` 每次复制基线后运行 `project cache-clear`，断言 checkpoints/cache 字节数为零；这只清除工程派生缓存。重复预览和检查点恢复每次都启动新进程，断言实际 `cache_hit.kind/tier/reused_steps`，不可能将进程内缓存或回退重放记作磁盘命中。

## 场景和前置状态

| 场景 | 固定工作量与状态 |
| --- | --- |
| version | `--version` |
| jpeg1080_resize_adjust | 1080p JPEG → bilinear 1280×720 → exposure=0.5 EV、saturation=1.1 → JPEG q90 |
| jpeg4k_adjust | 4K JPEG → 同一基础调整 → JPEG q90 |
| png1080_transparent | 1080p RGBA PNG → 同一缩放/调整 → PNG compression=6 |
| layers4k | 4K JPEG + 完整 4K RGBA PNG → layer_add、layer_set(screen, 0.7) → PNG 6 |
| blur1080_sigma12 | 1080p RGBA PNG → sigma=12、半径 36 的 73-tap 可分离高斯 → PNG 6 |
| project_first_commit | 已导入 1080p JPEG 的 r0 工程，每次重新复制，然后一次提交 20 个真实 adjust；导入成本单列在 setup，未混入首提交 |
| project_replay | 同一 r20 基线复位，清工程缓存，重放 20 步，导出全尺寸 PNG 6 |
| project_checkpoint | r20 检查点磁盘命中，复用 20、重算 0，导出全尺寸 PNG 6 |
| project_preview_repeat | r20 的 640×360 bilinear 预览已缓存，命中 preview/disk，复用 20、重算 0，重新编码 PNG 6 |
| project_revise18 | 每轮恢复原 r20 + r2/r17/r20 检查点；替换 r18 参数，复用 17、提交新 3 步 |
| project_revise3 | 每轮恢复同一原 r20 + 同一检查点；替换 r3 参数，复用 2、提交新 18 步 |
| layers4k_replay | 同一双层 r2 工程复位并清缓存，重放 2 步，导出全尺寸 PNG 6 |
| layers4k_checkpoint | 双层 r2 完整文档检查点磁盘命中，复用 2、重算 0，导出全尺寸 PNG 6 |
| layers4k_preview_repeat | r2 的 960×540 bilinear 预览命中 preview/disk，复用 2、重算 0，PNG 6 |
| layers4k_checkpoint_store | 每轮复位双层 r2 并清缓存，重放后保存完整检查点，断言 `disk_stored=true` |

20 步交替使用 exposure=0.15/-0.12、saturation=1.01/0.99，brightness=0、contrast=1；修改时使用 exposure=0.25、saturation=1.02。每次修改都从原 r20 出发，生成 r23 或 r38，历史不会跨轮积累。原始行包含复位后的 manifest SHA-256、实际重算 revision 数组、工程前后逻辑/分配字节及各目录占用。创建、检查点写入和预览预热的真实 CLI 参数、返回值及耗时也保留在 meta 的 setup 中。

## 优化及语义边界

变更前 release 二进制来自 `bbd387f08fe4ed2104b91aca61abf36947f4a5ef`，在产品修改前保存为 `target/performance/binaries/before`；SHA-256 为 `9a99a79b734fd37bfe51330837df72510ef1ace59cc3e2a727283f1d19aa779a`；after 二进制 SHA-256 为 `98ed3ff3ebf4aefcb1030c7742179a4d9221005dea7998b8502295487fb94f50`。基准始终执行保存的二进制。先有试跑和正式样本的阶段证据，再修改核心；编译和测试安排在正式采样之外。

实测和调用链共同定位到 `Project::restore`：它丢弃 `Pipeline::execute_document` 已合成的 raster，最后再次合成全部图层；命中完整检查点时也丢弃缓存画布再渲染。工程恢复修改保留这两个已有结果。续编时先释放旧画布，再执行后缀，返回最后执行块的画布；仍保留完整 Document 和所有逻辑 revision。校验源资产、缓存 key/checksum、文档预算及 expected_revision 的路径保持原有规则；新版本还会实测读取旧二进制生成的完整多层检查点。

另一个已测热点是 4K JPEG 解码阶段约 0.5 秒，其中原实现对每个 RGB 字节反复调用相同的 sRGB `powf` 转换。解码现在每次用原函数计算全部 256 个可能输入，随后查表；仅增加 1 KiB 栈空间，不量化或近似浮点工作数据。coverage 蒙版继续使用编码灰度乘 alpha 的独立公式。新增穷举回归对所有字节值及隐藏透明 RGB 比较原标量公式的 f32 bits，另外验证蒙版路径。

没有融合或重排操作，也没有改变浮点公式、颜色/alpha、混合、采样、滤波核、缩放或编码设置。相邻点操作融合需要保持每步 f32 舍入与可观察边界，本次未在这条路径引入变更。编码、缩放和线程策略保持一致，未为通过时间目标降低质量。

## 30 轮实测结果

每格耗时为 p50 / p95，单位 ms；RSS 为 30 次子进程峰值中的最大值（MiB）。逐次峰值的 p50/p95、全部阶段统计在 [comparison.json](performance/comparison.json)。

| 场景 | before ms | after ms | before / after 最大 RSS MiB |
| --- | ---: | ---: | ---: |
| blur1080_sigma12 | 1566.38 / 1670.21 | 1491.72 / 1674.59 | 132.16 / 132.29 |
| jpeg1080_resize_adjust | 291.29 / 313.57 | 223.74 / 252.64 | 104.29 / 104.29 |
| jpeg4k_adjust | 1340.27 / 1429.44 | 1076.87 / 1171.49 | 258.96 / 258.96 |
| layers4k | 3899.53 / 4002.06 | 3343.83 / 3494.81 | 674.54 / 674.54 |
| layers4k_checkpoint | 3948.56 / 4100.12 | 2716.09 / 2868.14 | 771.64 / 771.49 |
| layers4k_checkpoint_store | 4477.53 / 4604.95 | 2748.55 / 2806.40 | 801.17 / 801.16 |
| layers4k_preview_repeat | 251.08 / 266.89 | 250.19 / 264.78 | 24.90 / 24.89 |
| layers4k_replay | 5049.48 / 5172.67 | 3359.91 / 3425.70 | 677.64 / 677.64 |
| png1080_transparent | 434.55 / 457.94 | 369.42 / 390.37 | 104.17 / 104.29 |
| project_checkpoint | 478.67 / 496.07 | 478.23 / 520.45 | 69.12 / 69.12 |
| project_first_commit | 761.92 / 800.11 | 698.35 / 732.47 | 100.81 / 100.91 |
| project_preview_repeat | 111.86 / 117.94 | 112.63 / 124.93 | 13.15 / 13.14 |
| project_replay | 1119.38 / 1202.27 | 1061.51 / 1146.76 | 69.84 / 69.84 |
| project_revise18 | 236.93 / 253.80 | 230.70 / 247.90 | 100.29 / 100.29 |
| project_revise3 | 671.37 / 703.70 | 663.49 / 711.11 | 100.30 / 100.29 |
| version | 3.26 / 3.66 | 3.18 / 3.78 | 4.12 / 4.12 |

阶段热点（p50，ms；辅助素材加载也按现有 CLI 定义计入 process）：

| 场景 / 阶段 | before | after |
| --- | ---: | ---: |
| jpeg4k_adjust / decode_ms | 511.538 | 259.028 |
| jpeg4k_adjust / process_ms | 206.975 | 198.955 |
| jpeg4k_adjust / encode_ms | 569.917 | 572.983 |
| layers4k_replay / process_ms | 2753.867 | 1301.556 |
| layers4k_checkpoint / read_ms | 945.957 | 884.941 |
| layers4k_checkpoint / process_ms | 1179.214 | 0.009 |
| layers4k_checkpoint / encode_ms | 1740.812 | 1747.050 |
| layers4k_checkpoint_store / process_ms | 2766.185 | 1309.932 |
| layers4k_checkpoint_store / write_ms | 1116.129 | 1105.608 |

工程磁盘（after，每轮结束逻辑字节；完整原始行还给出分配字节及 assets/ops/cache/checkpoints 分项）：

| 场景 | 逻辑字节 | 复用 / 重算步骤 | 实际缓存 |
| --- | ---: | ---: | --- |
| layers4k_checkpoint | 413,722,749 | 2 / 0 | checkpoint/disk |
| layers4k_checkpoint_store | 405,428,229 | 0 / 2 | miss |
| layers4k_preview_repeat | 413,722,749 | 2 / 0 | preview/disk |
| layers4k_replay | 7,295,952 | 0 / 2 | miss |
| project_checkpoint | 103,932,184 | 20 / 0 | checkpoint/disk |
| project_first_commit | 712,504 | 0 / 20 | miss |
| project_preview_repeat | 103,932,184 | 20 / 0 | preview/disk |
| project_replay | 712,504 | 0 / 20 | miss |
| project_revise18 | 103,934,018 | 17 / 3 | checkpoint/disk |
| project_revise3 | 103,942,034 | 2 / 18 | checkpoint/disk |

测量时间（UTC）：before 2026-09-19T12:46:13Z 至 2026-09-19T12:59:46Z；after 2026-09-19T13:03:40Z 至 2026-09-19T13:14:13Z。

## 结果解释与未达门槛

建议目标按完整外部 wall p95 核对：`--version` **3.78 ms ≤ 30 ms**；1080p JPEG 缩放/调整 **252.64 ms ≤ 300 ms**（before 313.57 ms）；4K JPEG 调整 **1171.49 ms > 1000 ms**（before 1429.44 ms），仍超约 17.1%。没有将接近目标写成达标。

4K 调整的 decode p50 从 511.54 降到 259.03 ms，与减少重复 sRGB 转换一致；最终 encode p50 仍为 572.98 ms，process 为 198.96 ms。编码阶段包括线性像素转输出样本及 JPEG 编码，现有计时未进一步拆开；不能据此声称某个编码器内部函数占比。此次保持 JPEG q90、采样和单 CLI 线程预算，未优化编码或以降低质量通过目标。

双层 4K 完整重放 p95 **5172.67 → 3425.70 ms（下降 33.8%）**，磁盘检查点恢复 **4100.12 → 2868.14 ms（下降 30.0%）**，检查点写入场景 **4604.95 → 2806.40 ms（下降 39.1%）**。完整检查点命中的 process p50 **1179.214 → 0.009 ms**，实证不再重复合成；检查点序列化写入 p50 仍约 1.1 秒，没有改善磁盘带宽的承诺。快照读入的双份缓冲仍主导峰值内存，最高约 **801 MiB**，没有声称降低 RSS。

没有所有场景都变快的结论：单图检查点导出 p95 增加 4.9%、重复预览增加 5.9%、第 3 步修改增加 1.1%、version 增加 3.3%；这些路径没有预期的解码/多层合成收益。before / after 的 1 分钟 load average 范围分别为 2.61–4.84 / 2.42–5.33，虚拟机争用未隔离；不把这类小差异当成可推广的加速或确定回归。重复预览依然需要资产校验、读取浮点预览和重新编码；工程缓存不是已编码输出文件缓存。

大模糊 p95 **1674.59 ms**（before 1670.21 ms），保留 sigma=12、73-tap/f64 累加与 alpha 规则；它不适用普通 4K 调整的 1 秒建议目标。透明 PNG、双层合成与工程场景目前只建立可复跑基线，不编造新的产品 SLO。没有 Photoshop 或 libvips 的实测比较；所有改善仅指这次自身 release 二进制对照。

## 预算与复现

没有新增线程池；锁定依赖未启用 Rayon，并顺序启动一个 CLI。核心默认 max_buffer_bytes=1 GiB，属于准入预算，不能等同硬 RSS 限制；默认磁盘缓存 512 MiB、内存缓存/快照 scratch 256 MiB。双层 4K 完整检查点包含画布及两层 RGBA32F，像素部分 398,131,200 字节（约 379.69 MiB），还需元数据。序列化/恢复准入要求约两倍，因此默认预算实际返回 `disk_stored=false`；该结果保存在 meta，未作为命中样本。

4K 检查点相关场景显式 `--cache-memory-bytes 1073741824 --cache-disk-bytes 536870912`，不改变产品默认值。完整快照约 380 MiB；1080p 单图检查点约 31.64 MiB，r2/r17/r20 合计约 94.92 MiB。每次只保留一个可变 sample.pic，原始素材与 ops 不计入可淘汰缓存。最终表格给出实测峰值 RSS 和磁盘占用；操作系统缓存、Rust allocator 和 codec scratch 另有成本。

依赖：Linux、Python 3.11+ 标准库（使用 `hashlib.file_digest`）、GNU `/usr/bin/time`、当前 Rust 工具链，无需额外 Python 包。运行目录必须尚不存在；采样失败立即非零退出，不能把不完整结果当作验收。完整重跑会生成数 GiB 工程/输出，应预留约 4 GiB 额外空间；不在采样同时运行编译或测试。

存档 `docs/performance/*.meta.json` 中的绝对路径仅在本次原 worktree 尚存在时有效；它们是历史证据，不能作为长期可执行的像素复跑入口。仅重新计算历史统计不需要这些二进制或工程：

```sh
python3 scripts/performance.py summarize docs/performance/before.jsonl docs/performance/after.jsonl
python3 scripts/performance.py compare docs/performance/before.jsonl docs/performance/after.jsonl
```

以下是清理原 worktree 后也可复制执行的完整复跑，在包含本任务代码的仓库根目录运行。基线从固定提交导出到普通目录，不切换或修改任何分支；after 使用当前 checkout。全部编译先完成，再在同一新建目录/文件系统顺序测量两个保存的二进制。像素脚本绑定**这次新采样生成的** meta，不引用存档绝对路径。

```sh
set -eu
mkdir -p "$PWD/target/performance"
pic_perf_run=$(mktemp -d "$PWD/target/performance/recheck.XXXXXX")
pic_perf_cpu=0
mkdir "$pic_perf_run/before-source" "$pic_perf_run/binaries"
git archive --format=tar --output="$pic_perf_run/before-source.tar" \
  bbd387f08fe4ed2104b91aca61abf36947f4a5ef
tar -xf "$pic_perf_run/before-source.tar" -C "$pic_perf_run/before-source"
cargo build --locked --release \
  --manifest-path "$pic_perf_run/before-source/Cargo.toml" \
  --target-dir "$pic_perf_run/before-build"
cargo build --locked --release
cargo build --locked --release -p pic-cli --example performance_fixture
cp "$pic_perf_run/before-build/release/pic-cli" "$pic_perf_run/binaries/before"
cp "$PWD/target/release/pic-cli" "$pic_perf_run/binaries/after"
pic_perf_fixture="$PWD/target/release/examples/performance_fixture"

python3 scripts/performance.py run \
  --binary "$pic_perf_run/binaries/before" --fixture-tool "$pic_perf_fixture" \
  --work-dir "$pic_perf_run/before-work" --output "$pic_perf_run/before.jsonl" \
  --label before --cpu "$pic_perf_cpu"
python3 scripts/performance.py run \
  --binary "$pic_perf_run/binaries/after" --fixture-tool "$pic_perf_fixture" \
  --work-dir "$pic_perf_run/after-work" --output "$pic_perf_run/after.jsonl" \
  --label after --cpu "$pic_perf_cpu"
python3 scripts/performance.py compare \
  "$pic_perf_run/before.jsonl" "$pic_perf_run/after.jsonl" \
  > "$pic_perf_run/comparison.json"
python3 scripts/performance_pixels.py \
  --before "$pic_perf_run/before.meta.json" --after "$pic_perf_run/after.meta.json" \
  --fixture-tool "$pic_perf_fixture" --work-dir "$pic_perf_run/pixel-check" \
  --output "$pic_perf_run/pixels.json"
```

`run --output .../before.jsonl` 自动写入同目录的 `before.meta.json`，并绑定该次二进制、工程与输出的实际路径；after 同理。保留新目录到像素检查结束，再保存新的 jsonl/meta/comparison/pixels 作为该次记录。二进制哈希受工具链、路径和平台影响，新的 meta 会记录实际哈希，不要求与存档二进制哈希相同。素材/操作参数在两次采样之间仍须一致。

若 CPU 0 不在当前允许 affinity 中，把 `pic_perf_cpu` 设为允许的同一个 CPU；应在相同硬件/编译配置、相同负载下比较。`--scenario layers4k_replay --scenario layers4k_checkpoint` 可用于仅复跑性能子集，默认每个仍为 30 次；完整像素核对脚本需要上面生成的全套场景。`--runs 1 --smoke` 只用于检查脚本与缓存条件，不能作为性能验收。

本次原二进制位于忽略目录 `target/performance/binaries/`，不是长期交付物；原始样本、meta、汇总、校验日志及像素核对结果均作为 Git 跟踪文件保存。稳定 CI 使用 Rust 像素、重放、检查点及历史回归断言，不使用 wall time 硬门槛。性能脚本默认仅报告差异；`compare --max-p95-ratio 1.15` 是受控同机重复测量时可选的人工回归护栏，不能将共享 CI 的单次波动直接视为失败。比较同时核对固定素材/操作参数和缓存预算，仍应查看两次新 meta 的硬件、编译器、负载及完整 argv。

像素脚本使用 meta 中的实际二进制/产物路径并验证二进制 hash；重跑采样时以新 meta 为输入。它解码比较所有渲染场景，比较全重放/检查点/直接多层输出，逐一检查 r0–r20 的文档、历史和像素，并独立复位修改第 18/3 步后核对前后版本及完整重放。库回归另外逐 f32 bit 核对含 HDR/负 RGB/alpha 的任意图层前缀、分块重放和磁盘/内存检查点，防止 RGBA8 导出掩盖工作精度变化。

## 验证与交付文件

前后各 480 次正式新进程成功样本（16×30），另各 16 次 warmup。逐次原始数据为 [before.jsonl](performance/before.jsonl) / [after.jsonl](performance/after.jsonl)；环境、素材哈希、参数及 setup 证据为 [before.meta.json](performance/before.meta.json) / [after.meta.json](performance/after.meta.json)。[comparison.json](performance/comparison.json) 可由 compare 命令重新生成；[sample-audit.json](performance/sample-audit.json) 登记完整轮次、状态复位、前后参数和编码产物哈希核对。[resource-check.json](performance/resource-check.json) 记录独立的线程观察。

[像素证据](performance/pixels.json)：45 组实际解码比较，共 574,387,200 个 RGBA8 通道，差异全部为零；r0–r20 的 Document/历史全部一致，修改第 18/3 步后的前缀恢复与完整重放一致，新二进制读取旧二进制的完整多层检查点也命中且一致。正式采样的每个渲染场景，前后所有输出的编码字节 SHA-256 也相同；解码比较仍独立执行，未用压缩字节相同替代像素验证。

[完整校验日志](performance/validation.json) 记录实际命令、退出码和输出：

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：129 项通过。
- `cargo build --release`：通过。
- `cargo test --release -p pic-core`：80 项通过，含全字节转换的浮点位精度、完整图层、蒙版、文字、重放及缓存一致性。
- `PIC_CLI_BIN="$PWD/target/release/pic-cli" cargo test -p pic-cli --test cli --test project`：49 项通过，含 expected_revision 并发、缓存损坏/缺失、历史观察和继续编辑。
- `git diff --check`：通过；提交前再次检查最终文档/队列变更。

新增 `every_layered_prefix_matches_direct_pixels_across_chunks_and_checkpoint_tiers` 对含 HDR/负 RGB、蒙版、旋转、overlay、不透明度、反相和裁剪的每个前缀逐 f32 bit 比较直接执行、多块重放、内存/磁盘检查点，以及命中检查点后继续执行的画布与完整 Document。新增 `byte_transfer_lookup_matches_scalar_float_bits_and_keeps_coverage_separate` 穷举输入字节并保留独立 coverage 语义。没有外部阻塞；4K 调整未达到建议 1 秒和 libvips 对照缺失是已披露的性能/对照限制。
