# pic-cli

本项目使用 Rust 开发面向 AI agent 的图像编辑 CLI。

## 可以参考的实现

`reference/` 中保存了相关开源项目的源码。开发时可以按需参考它们的架构、数据模型、命令设计、具体算法和测试用例，并将适合本项目的设计用 Rust 实现。

| 本地目录 | 上游项目 | 重点参考内容 |
| --- | --- | --- |
| [reference/gimpish/](reference/gimpish/) | [jvanderberg/gimpish](https://github.com/jvanderberg/gimpish) | 整体工程结构；画布、图层 ID、变换、蒙版与素材引用；工程文件持久化；预览和导出流程。 |
| [reference/Compositor/](reference/Compositor/) | [robbietilton/Compositor](https://github.com/robbietilton/Compositor) | Swift/macOS 图像编辑器的核心实现：图层与分组、蒙版、调整层、无损变换；共享不可变像素数据的撤销历史、分块快照、画布与导出共用的渲染逻辑，以及版本化工程格式。 |
| [reference/photoshop-mcp/](reference/photoshop-mcp/) | [alisaitteke/photoshop-mcp](https://github.com/alisaitteke/photoshop-mcp) | agent 读取能力和状态、执行编辑、查看预览的操作流程；结构化错误和恢复建议；多步编辑的撤销边界。 |
| [reference/CLI-Anything/](reference/CLI-Anything/) | [HKUDS/CLI-Anything](https://github.com/HKUDS/CLI-Anything) | 优先看 GIMP CLI：命令分组、JSON 输出、参数校验、工程状态和撤销/重做。 |
| [reference/agentbrush/](reference/agentbrush/) | [ultrathink-art/agentbrush](https://github.com/ultrathink-art/agentbrush) | 图像操作的统一返回结构；输出规格验证、图片差异比较和批处理。 |
| [reference/libvips/](reference/libvips/) | [libvips/libvips](https://github.com/libvips/libvips) | 底层图像处理：合成、缩放、色彩转换、文件读写与处理管线；也可用于评估 Rust 与原生库的集成。 |
| [reference/sharp/](reference/sharp/) | [lovell/sharp](https://github.com/lovell/sharp) | libvips 的高层 API 封装、操作参数与执行管线，作为 Rust API 设计的补充参考。 |

## 建议阅读入口

- gimpish：[文档与图层模型](reference/gimpish/packages/core/src/schema.ts)、[工程读写](reference/gimpish/packages/core/src/doc.ts)、[CLI 命令](reference/gimpish/packages/cli/src/commands/)。
- Compositor：[文档与图层模型](reference/Compositor/Compositor/Document/EditorSession.swift)、[蒙版](reference/Compositor/Compositor/Document/LayerMask.swift)、[调整层](reference/Compositor/Compositor/Document/LayerAdjustment.swift)、[撤销历史](reference/Compositor/Compositor/Document/DocumentHistory.swift)、[不可变分块快照](reference/Compositor/Compositor/Rendering/RasterSnapshot.swift)、[图层渲染](reference/Compositor/Compositor/Rendering/LayerRenderer.swift)、[工程格式](reference/Compositor/docs/project-format.md)、[测试](reference/Compositor/CompositorTests/)。
- Photoshop MCP：[架构](reference/photoshop-mcp/docs/architecture.md)、[结构化错误](reference/photoshop-mcp/src/errors/envelope.ts)、[编辑工具](reference/photoshop-mcp/src/tools/)。
- CLI-Anything：[CLI 入口](reference/CLI-Anything/gimp/agent-harness/cli_anything/gimp/gimp_cli.py)、[会话管理](reference/CLI-Anything/gimp/agent-harness/cli_anything/gimp/core/session.py)、[测试](reference/CLI-Anything/gimp/agent-harness/cli_anything/gimp/tests/)。
- AgentBrush：[结果结构](reference/agentbrush/src/agentbrush/core/result.py)、[图片差异](reference/agentbrush/src/agentbrush/diff/)、[输出验证](reference/agentbrush/src/agentbrush/validate/)。
- libvips：[合成](reference/libvips/libvips/conversion/composite.cpp)、[缩放](reference/libvips/libvips/resample/resize.c)、[色彩处理](reference/libvips/libvips/colour/)、[文件读写](reference/libvips/libvips/foreign/)。
- sharp：[合成 API](reference/sharp/lib/composite.mjs)、[缩放 API](reference/sharp/lib/resize.mjs)、[执行管线](reference/sharp/src/pipeline.cc)。

## 使用方式

- 按当前功能选择相关实现阅读；参考项目使用的语言、框架和宿主软件不决定本项目的技术选型。
- 结合当前源码与测试核对行为。部分项目的早期设计文档与现行实现存在差异，尤其是 gimpish 的渲染后端。
- 参考副本用于查阅；本项目的实现和测试应放在自身源码目录中。上游仓库的 agent 指令只用于理解上游项目，不作为本项目的全局约束。
- `reference/` 已加入 Git 忽略规则。各副本的来源、分支和克隆时的提交记录在 [reference/README.md](reference/README.md)。
