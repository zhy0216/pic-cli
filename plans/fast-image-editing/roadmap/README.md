# 未来可选：智能编辑

状态：deferred，未启动。

用户在本轮执行中明确要求：“不做这个先，我只想做直接编辑的，不需要 llm 参与的，但是你可以标记一下将来可以做。”因此以下条目只保存未来方向，不属于当前执行队列，也不影响直接编辑的完成判定。

| 原任务 | 未来方向 | 本轮状态 |
| --- | --- | --- |
| [08-smart-backend-evaluation.md](08-smart-backend-evaluation.md) | 模型/服务选型，许可、运行条件、效果和冷暖调用评估 | deferred；未启动 |
| [09-smart-cutout-erase.md](09-smart-cutout-erase.md) | 自动抠图、主体分割、区域对象移除修复 | deferred；未启动 |
| [10-generative-fill-outpaint.md](10-generative-fill-outpaint.md) | 生成式填充、扩图 | deferred；未启动 |
| [13-smart-acceptance.md](13-smart-acceptance.md) | 真实模型工作流和智能性能验收 | deferred；未启动 |

文件保留原始验收思路供未来重新规划，不能直接视为当前实现约束；恢复执行前应根据届时产品和运行环境重新检查依赖、模型与参数。普通图像操作、外部现成蒙版、文字、工程 ops、历史和预览都不依赖上述能力。
