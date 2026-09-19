# 可运行 JSON 示例

完整步骤在 [agent 指南](../docs/agent-guide.md)，可用 [打包验收](../docs/packaging.md) 一次复跑。脚本先生成固定小素材，再把本目录 JSON 复制到临时 cwd 的 `recipes/`；因此素材路径的 `../` 有明确含义，不依赖仓库 cwd。

| 文件 | 用法与约束 |
| --- | --- |
| photo.json | crop → adjust，输入至少 168×104，示例生成 192×128 |
| followup.json | 一步 sharpen，用观察到的 revision 继续编辑 |
| bindings.json | 对 photo 两步模板重新绑定新输入、目标及全部参数；新 crop 最少需要 144×92 |
| layers.json | run/apply 使用的完整图层管线，绑定背景外的主体、灰度蒙版和静态 TTF；包含组、文字、调整层与 clip。当前不能由 template-run 换图 |

图片为 `agent_fixtures` 通过整数坐标生成的测试样本，无外部图片来源。文字测试使用 [DejaVu Sans 的来源与许可](../tests/fonts/README.md)，没有系统字体回退。字体文件和版权通知随验收包分发。
