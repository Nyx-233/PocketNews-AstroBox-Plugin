# 接口来源

- 开发文档：https://abox.run/docs/plugin-development/v4
- 更新说明：https://abox.run/docs/plugin-development/v4/whats-new
- 语言支持：https://abox.run/docs/plugin-development/v4/getstarted/language
- 官方 WIT：https://github.com/AstralSightStudios/AstroBox-Plugin-WIT
- 对应修订：`c08d9efe2e950f550f0f9838364618ebc2e8fa88`

`deps/astrobox-psys-host-v4.wit` 和 `deps/astrobox-psys-plugin-v4.wit` 保留该修订的原文。Windows 换行符归一化为 LF 后，SHA-256 分别为：

```text
e43cf7d3d3e7aab73212b01325d551441be20a835e101c4f28a892d23e46b39b  deps/astrobox-psys-host-v4.wit
72380559aa31881c160b445569f423d35ba5dca9753f687f80025d08d5e21565  deps/astrobox-psys-plugin-v4.wit
```

`main.wit` 仅保留 v4 世界，`psys-world-v4` 的导入、导出与该修订一致；不引入 Level 2/3 包。新闻插件不调用 CanopusConnect、HTTP 服务器、浏览器等可选接口，也不为这些接口申请权限。

插件使用 Rust `wit-bindgen` 生成 v4 绑定。生命周期和事件导出均为 `async fn`；等待型宿主调用直接 `.await`；UI Builder 与 `ui::render` 按 WIT 保持同步。宿主的 v4 UI 实现复用 UI Builder 数据结构，并不要求插件改为 HTML 页面。

清单声明 `api_level: 4`、`wasi_version: 3`。构建目标为 `wasm32-wasip2`，因为 v4 文档明确保留 WASI Preview 2 标准库接口；现有 HTTP 客户端使用这些接口，不将编译目标误写成清单版本。

界面使用 `astrobox:psys-host-v4/ui`，保留普通控件的默认样式，不混用 `astrobox:psys-host/ui` 或 `ui-v3`。组件专属属性若未核实，不按其他前端框架的用法猜测。
