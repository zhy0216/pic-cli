# 可分发排版测试字体

`DejaVuSans.ttf` 是未修改的 DejaVu Sans 2.37（本机 Ubuntu `fonts-dejavu-core` 提供）。上游：[DejaVu Fonts](https://dejavu-fonts.github.io/)，[许可证原文](https://github.com/dejavu-fonts/dejavu-fonts/blob/master/LICENSE)。本目录的 [LICENSE-DejaVu.txt](LICENSE-DejaVu.txt) 保留 Bitstream 版权、商标、许可和免责声明；DejaVu 的增补为 public domain。随字体分发此许可通知；示例输出目录也会复制它。

SHA-256：`ae7b7855e115a5966d8b1b3f80f254ccc117ec86f9965e202ee2940453837280`，759720 字节。保留原字体名称和完整字体，未进行子集化或重命名。

测试通过显式路径或 `include_bytes!` 绑定此文件，不依赖系统字体目录。CLI 不自带默认字体或搜索系统字体；用户传入的字体会作为工程资产保存。实测样例为 `Café ffi`、`e\u0301`、`Ωμέγα`、`Привет`，验证拉丁连字、字距、组合附加符号及换行/对齐；不宣称该字体或当前排版器支持全部 Unicode。此字体不含示例中文 `中`（U+4E2D），必须返回 `missing_glyph`，不能替换为方框或其他字体。
