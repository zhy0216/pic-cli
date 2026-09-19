difficulty: hard
agent: inherit

# 自包含工程与持久化历史

## T1 · 资产、不可变 ops 和跨进程原子提交

前置依赖：03-adjustments-filters.md。

要做什么：实现 .pic 目录的 manifest/ops/assets，内容哈希去重导入素材，结构版本与操作语义版本验证。记录已规范化语义 ops、稳定目标/op ID、base/output revision 和组提交边界；工程状态只能由原图/已提交 ops/必需结果资产恢复。实现 project create/apply/inspect/export、线性 undo/redo、指定已提交步骤读取和继续编辑；旧 revision 不可原地改写，旧 redo 路径失效但历史只读可追溯。发布前校验 expected_revision，跨进程写锁或等价原子冲突控制；失败不得发布半套历史。工程路径与素材引用不得逃逸预期资产范围。

预计修改：pic-core document、资产/历史/存储模块与 pipeline 接口，pic-cli project 子命令及文档、集成测试，本 todo/README。

验收条件：

- [ ] 单图导入、组提交、多步中间 revision 读取、撤销/重做、从旧步骤续编和导出全部可跨独立 CLI 进程完成。
- [ ] 查询、失败操作和预览不进入编辑历史；只有成功发布的 ops 成为权威状态。
- [ ] 移动工程并删除原始导入文件仍能恢复；素材去重，丢失/损坏素材、未知 schema/op 版本有清晰错误。
- [ ] 并发 expected_revision 冲突只有一个发布成功；执行/资产写入/manifest 发布失败不产生部分可见历史，保留原有效工程。
- [ ] 直接管线与从工程 ops 全重放的解码像素一致，完整仓库校验通过。

验证：完整仓库校验；跨进程集成测试、并发提交、故障注入/受控 IO 失败、素材移动与哈希损坏测试。
